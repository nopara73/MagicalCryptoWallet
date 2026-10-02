using NBitcoin;
using Nito.AsyncEx;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.WabiSabi.Client;

namespace MagicalCryptoWallet.Wallets;

public enum WalletSessionState { Unconfigured, Loading, Syncing, Ready, Offline, Faulted, Stopping }
public record WalletSessionSnapshot(WalletSessionState State, bool HasCachedData, uint? SyncHeight, uint? TargetHeight,
	bool CoinJoinRequiresAuthorization, bool PublicMetadataRequiresAuthorization, string? Error = null)
{
	public bool IsSynchronized => State == WalletSessionState.Ready;
	public string Status => State.ToString();
}

/// <summary>Owns the configured wallet for the application lifetime, independently of any window.</summary>
public sealed class WalletSession
{
	private readonly Lock _gate = new();
	private readonly AsyncLock _lifecycle = new();
	private readonly WalletFactory _createWallet;
	private readonly Func<bool> _isConnected;
	private readonly Func<CancellationToken, Task>? _recoverStorage;
	private readonly CancellationTokenSource _stopping = new();
	private Wallet? _wallet;
	private WalletAuthorization? _coinJoinAuthorization;
	private Task? _startTask;
	private Task? _monitorTask;
	private Func<CancellationToken, Task<IAsyncDisposable>>? _recoveryGuard;
	private Task? _stopTask;
	private bool _storesReady;
	private bool _disposed;
	private bool _hasCachedData;
	private bool _missingPublicMetadata;
	private WalletSessionSnapshot _snapshot = new(WalletSessionState.Unconfigured, false, null, null, false, false);

	public WalletSession(Network network, WalletDirectories walletDirectories, WalletFactory createWallet, Func<bool>? isConnected = null, Func<CancellationToken, Task>? recoverStorage = null)
	{
		Network = network;
		WalletDirectories = walletDirectories;
		_createWallet = createWallet;
		_isConnected = isConnected ?? (() => true);
		_recoverStorage = recoverStorage;
		ReadConfiguredWallet();
	}
	public Network Network { get; }
	public WalletDirectories WalletDirectories { get; }
	public event EventHandler<Wallet>? WalletConfigured;
	public event EventHandler<WalletSessionSnapshot>? Changed;
	public event EventHandler<EventArgs>? OperationAuthorized;
	public WalletSessionSnapshot Snapshot { get { lock (_gate) { return _snapshot; } } }
	public IDisposable Subscribe(Action<WalletSessionSnapshot> observer)
	{
		void OnChanged(object? sender, WalletSessionSnapshot snapshot) => observer(snapshot);
		lock (_gate)
		{
			Changed += OnChanged;
			try { observer(_snapshot); } catch { Changed -= OnChanged; throw; }
		}
		return new Subscription(() => { lock (_gate) { Changed -= OnChanged; } });
	}
	private sealed class Subscription(Action unsubscribe) : IDisposable
	{
		private Action? _unsubscribe = unsubscribe;
		public void Dispose() => Interlocked.Exchange(ref _unsubscribe, null)?.Invoke();
	}
	public Wallet? GetWallet() { lock (_gate) { return _wallet; } }
	public bool HasWallet() => GetWallet() is not null;
	public IKeyChain? CoinJoinKeyChain { get { lock (_gate) { return _coinJoinAuthorization is { } scope ? new KeyChain(scope) : null; } } }

	private void ReadConfiguredWallet()
	{
		try
		{
			if (WalletDirectories.ResolveConfiguredWalletFile() is not { } filename)
			{
				_snapshot = new(WalletSessionState.Unconfigured, false, null, null, false, false);
				return;
			}
			_wallet = _createWallet(KeyManager.FromFile(filename));
			WalletDirectories.PersistConfiguredFile(filename);
			_missingPublicMetadata = !_wallet.KeyManager.IsWatchOnly && _wallet.KeyManager.TaprootExtPubKey is null;
			_snapshot = new(WalletSessionState.Loading, false, null, null, RequiresCoinJoinAuthorization(), _missingPublicMetadata);
		}
		catch (Exception ex)
		{
			_wallet?.Dispose();
			_wallet = null;
			_snapshot = new(WalletSessionState.Faulted, false, null, null, false, false, ex.Message);
			Logger.LogError(ex);
		}
	}
	public void EnsureCanConfigure()
	{
		lock (_gate)
		{
			ObjectDisposedException.ThrowIf(_disposed, this);
			if (_wallet is not null || _snapshot.State == WalletSessionState.Faulted)
			{
				throw new InvalidOperationException("A wallet is already configured. Restore its file before starting the application.");
			}
		}
	}
	public Wallet Configure(KeyManager keyManager)
	{
		Wallet wallet;
		WalletSessionSnapshot configured;
		lock (_gate)
		{
			EnsureCanConfigure();
			var expectedPath = WalletDirectories.NewWalletFilePath;
			if (keyManager.FilePath is not { } path || !string.Equals(Path.GetFullPath(path), Path.GetFullPath(expectedPath), OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal))
			{
				throw new InvalidOperationException("The wallet draft must use the application wallet file.");
			}
			wallet = _createWallet(keyManager);
			try
			{
				WalletDirectories.Commit(keyManager);
				_wallet = wallet;
				_missingPublicMetadata = !keyManager.IsWatchOnly && keyManager.TaprootExtPubKey is null;
				configured = new(WalletSessionState.Loading, false, null, null, RequiresCoinJoinAuthorization(), _missingPublicMetadata);
			}
			catch (Exception ex)
			{
				wallet.Dispose();
				if (File.Exists(WalletDirectories.NewWalletFilePath) || File.Exists(WalletDirectories.SetupJournalPath))
				{ Publish(_snapshot with { State = WalletSessionState.Faulted, Error = ex.Message }); }
				throw;
			}
		}
		WalletConfigured.SafeInvoke(this, wallet);
		Publish(configured);
		StartIfReady();
		return wallet;
	}
	/// <summary>Starts after local stores initialize. Does not wait for network synchronization.</summary>
	public Task InitializeAsync(CancellationToken cancellationToken = default)
	{
		cancellationToken.ThrowIfCancellationRequested();
		lock (_gate)
		{
			ObjectDisposedException.ThrowIf(_disposed, this);
			_storesReady = true;
			_monitorTask ??= MonitorAsync(_stopping.Token);
		}
		StartIfReady();
		return Task.CompletedTask;
	}
	private void StartIfReady()
	{
		lock (_gate)
		{
			if (!_storesReady || _disposed || _wallet is null || _startTask is not null) { return; }
			var wallet = _wallet;
			_startTask = Task.Run(() => StartCoreAsync(wallet, _stopping.Token));
		}
	}
	private async Task StartCoreAsync(Wallet wallet, CancellationToken cancel)
	{
		using (await _lifecycle.LockAsync(cancel).ConfigureAwait(false))
		{
			try
			{
				wallet.InitializeLocalState();
				lock (_gate) { _hasCachedData = true; }
				if (!wallet.KeyManager.IsWatchOnly)
				{
					try { AuthorizeCoinJoin(""); }
					catch (System.Security.SecurityException) { /* A protected wallet synchronizes using its public accounts. */ }
				}
				UpdateStatus();
				await wallet.StartAsync(cancel).ConfigureAwait(false);
				UpdateStatus();
			}
			catch (OperationCanceledException) when (cancel.IsCancellationRequested) { }
			catch (Exception ex)
			{
				Publish(Snapshot with { State = WalletSessionState.Faulted, Error = ex.Message });
				Logger.LogError(ex);
			}
		}
	}
	internal IDisposable RegisterRecoveryGuard(Func<CancellationToken, Task<IAsyncDisposable>> guard)
	{
		lock (_gate) { _recoveryGuard = guard; }
		return new Subscription(() => { lock (_gate) { if (_recoveryGuard == guard) { _recoveryGuard = null; } } });
	}
	public async Task RetryAsync(CancellationToken cancel = default)
	{
		using (await _lifecycle.LockAsync(cancel).ConfigureAwait(false))
		{
			if (Snapshot.State != WalletSessionState.Faulted) { return; }
			if (!_storesReady && _recoverStorage is { } recover)
			{
				try { await recover(cancel).ConfigureAwait(false); }
				catch (OperationCanceledException) when (cancel.IsCancellationRequested) { throw; }
				catch (Exception ex) { ReportInitializationFailure(ex); return; }
				lock (_gate) { _storesReady = true; _monitorTask ??= MonitorAsync(_stopping.Token); }
			}
			var recoveryScope = _recoveryGuard is { } guard ? await guard(cancel).ConfigureAwait(false) : EmptyRecoveryGuard.Instance;
			await using var coinJoinGuard = recoveryScope.ConfigureAwait(false);
			if (_wallet is { } old)
			{
				try { await old.StopAsync(cancel).ConfigureAwait(false); }
				finally { old.Dispose(); }
			}
			lock (_gate)
			{
				ObjectDisposedException.ThrowIf(_disposed, this);
				_coinJoinAuthorization?.Dispose();
				_coinJoinAuthorization = null;
				_wallet = null;
				_hasCachedData = false;
				_startTask = null;
				var previous = _snapshot;
				_missingPublicMetadata = false;
				ReadConfiguredWallet();
				var recovered = _snapshot;
				_snapshot = previous;
				Publish(recovered);
			}
			if (_wallet is { } current) { WalletConfigured.SafeInvoke(this, current); }
			Publish(Snapshot);
		}
		StartIfReady();
	}
	public bool AuthorizeCoinJoin(string password)
	{
		var wallet = GetWallet() ?? throw new InvalidOperationException("No wallet is configured.");
		using var scope = WalletAuthorization.Create(wallet.KeyManager, password);
		AuthorizeCoinJoin(scope);
		return scope.CompatibilityPasswordUsed;
	}
	public void AuthorizeCoinJoin(WalletAuthorization scope)
	{
		lock (_gate)
		{
			ObjectDisposedException.ThrowIf(_disposed, this);
			RefreshPublicAccounts(scope);
			_coinJoinAuthorization ??= scope.Retain();
		}
		UpdateStatus();
	}
	/// <summary>Keep a separate CoinJoin authorization after any successful interactive or RPC authorization.</summary>
	public void CompleteOperationAuthorization(WalletAuthorization scope)
	{
		AuthorizeCoinJoin(scope);
		OperationAuthorized.SafeInvoke(this, EventArgs.Empty);
	}
	private void RefreshPublicAccounts(WalletAuthorization scope)
	{
		var wallet = GetWallet() ?? throw new InvalidOperationException("No wallet is configured.");
		if (!ReferenceEquals(wallet.KeyManager, scope.KeyManager)) { throw new InvalidOperationException("Authorization belongs to a different wallet session."); }
		if (_missingPublicMetadata && wallet.KeyManager.TaprootExtPubKey is not null)
		{
			wallet.KeyManager.GetKeys();
			wallet.WalletFilterProcessor.RequestRescan((uint)(wallet.KeyManager.GetBirthHeight() ?? 0));
			wallet.KeyManager.ToFile();
			_missingPublicMetadata = false;
		}
		UpdateStatus();
	}
	public void EnsureReady()
	{
		if (!Snapshot.IsSynchronized) { throw new InvalidOperationException("The wallet must finish synchronizing before this operation."); }
	}
	public void ReportInitializationFailure(Exception error) => Publish(Snapshot with { State = WalletSessionState.Faulted, Error = error.Message });
	private bool RequiresCoinJoinAuthorization() => _wallet is { KeyManager.IsWatchOnly: false } && _coinJoinAuthorization is null;
	private async Task MonitorAsync(CancellationToken cancel)
	{
		try
		{
			while (!cancel.IsCancellationRequested)
			{
				UpdateStatus();
				await Task.Delay(500, cancel).ConfigureAwait(false);
			}
		}
		catch (OperationCanceledException) when (cancel.IsCancellationRequested) { }
	}
	private void UpdateStatus()
	{
		WalletSessionSnapshot next;
		lock (_gate)
		{
			if (_disposed || _wallet is null || _snapshot.State == WalletSessionState.Faulted) { return; }
			if (_wallet.WalletFilterProcessor.ExecuteTask is { IsFaulted: true } worker)
			{
				Publish(_snapshot with { State = WalletSessionState.Faulted, Error = worker.Exception?.GetBaseException().Message });
				return;
			}
			var headers = _wallet.FilterHeaderChain;
			var target = headers.Tip is null ? (uint?)null : Math.Max(headers.ServerTipHeight, headers.TipHeight);
			var height = (uint)_wallet.KeyManager.GetBestHeight();
			var ready = _hasCachedData && _wallet.InitialSynchronizationFinished.IsCompletedSuccessfully && target is { } h && height >= h && headers.HashesLeft == 0 && !_missingPublicMetadata && !_wallet.WalletFilterProcessor.RescanPending;
			var state = !_hasCachedData ? WalletSessionState.Loading : (!_isConnected() || _wallet.WalletFilterProcessor.WaitingForBlock) ? WalletSessionState.Offline : ready ? WalletSessionState.Ready : WalletSessionState.Syncing;
			next = new(state, _hasCachedData, _hasCachedData ? height : null, target, RequiresCoinJoinAuthorization(), _missingPublicMetadata);
			Publish(next);
		}
	}
	private void Publish(WalletSessionSnapshot snapshot)
	{
		lock (_gate)
		{
			if (_snapshot == snapshot || (_disposed && snapshot.State != WalletSessionState.Stopping)) { return; }
			_snapshot = snapshot;
			Changed.SafeInvoke(this, snapshot);
		}
	}
	public Task StopAsync(CancellationToken cancel)
	{
		lock (_gate) { return _stopTask ??= StopCoreAsync(cancel); }
	}
	private async Task StopCoreAsync(CancellationToken cancel)
	{
		lock (_gate) { _disposed = true; }
		Publish(Snapshot with { State = WalletSessionState.Stopping });
		try
		{
			await _stopping.CancelAsync().ConfigureAwait(false);
			if (_startTask is { } start) { await start.WaitAsync(cancel).ConfigureAwait(false); }
			if (_monitorTask is { } monitor) { await monitor.WaitAsync(cancel).ConfigureAwait(false); }
			if (_wallet is { } wallet) { await wallet.StopAsync(cancel).ConfigureAwait(false); }
		}
		finally
		{
			_wallet?.Dispose();
			_coinJoinAuthorization?.Dispose();
			_coinJoinAuthorization = null;
			_stopping.Dispose();
		}
	}
	public void SetMaxBestHeight(uint bestHeight) => GetWallet()?.KeyManager.SetMaxBestHeight(bestHeight);
	private sealed class EmptyRecoveryGuard : IAsyncDisposable
	{
		public static readonly EmptyRecoveryGuard Instance = new();
		public ValueTask DisposeAsync() => ValueTask.CompletedTask;
	}
	public ChainHeight? GetBirthHeight() => GetWallet()?.KeyManager.GetBirthHeight();
	public ChainHeight? GetBestHeight() => GetWallet()?.KeyManager.GetBestHeight();
}
