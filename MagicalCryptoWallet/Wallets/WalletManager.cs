using NBitcoin;
using Nito.AsyncEx;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Models;
using static MagicalCryptoWallet.Logging.LoggerTools;

namespace MagicalCryptoWallet.Wallets;

public class WalletManager
{
	private readonly Lock _lock = new();
	private readonly AsyncLock _startStopWalletLock = new();
	private readonly CancellationTokenSource _cancelTasks = new();
	private readonly CancellationToken _cancelTasksToken;
	private readonly WalletFactory _createWallet;
	private Wallet? _wallet;
	private bool _disposed;

	public WalletManager(Network network, WalletDirectories walletDirectories, WalletFactory createWallet)
	{
		Network = network;
		WalletDirectories = walletDirectories;
		_createWallet = createWallet;
		_cancelTasksToken = _cancelTasks.Token;

		if (walletDirectories.GetConfiguredWalletName() is { } walletName)
		{
			// A missing or corrupt configured wallet must never silently open a different wallet.
			var wallet = _createWallet(KeyManager.FromFile(walletDirectories.GetWalletFilePaths(walletName + ".json")));
			try
			{
				walletDirectories.SetConfiguredWalletName(walletName);
				_wallet = wallet;
			}
			catch
			{
				wallet.Dispose();
				_cancelTasks.Dispose();
				throw;
			}
		}
	}

	public event EventHandler<Wallet>? WalletAdded;
	public Network Network { get; }
	public WalletDirectories WalletDirectories { get; }

	public Wallet? GetWallet()
	{
		lock (_lock)
		{
			return _wallet;
		}
	}

	public bool HasWallet() => GetWallet() is not null;

	public void EnsureCanAddWallet()
	{
		lock (_lock)
		{
			ObjectDisposedException.ThrowIf(_disposed, this);
			if (_wallet is not null)
			{
				throw new InvalidOperationException("A wallet is already configured. Magical Crypto Wallet supports one wallet per data directory.");
			}
		}
	}

	public Wallet AddWallet(KeyManager keyManager)
	{
		Wallet wallet;
		lock (_lock)
		{
			// Reject extra wallets before constructing one or writing its keys, including concurrent RPC requests.
			EnsureCanAddWallet();
			var expectedPath = WalletDirectories.GetWalletFilePaths(keyManager.WalletName + ".json");
			if (keyManager.FilePath is not { } filePath || !string.Equals(Path.GetFullPath(filePath), Path.GetFullPath(expectedPath), OperatingSystem.IsWindows() ? StringComparison.OrdinalIgnoreCase : StringComparison.Ordinal))
			{
				throw new InvalidOperationException("The wallet file must be inside the application wallet directory.");
			}
			if (File.Exists(expectedPath))
			{
				throw new InvalidOperationException("A wallet file already exists at the requested path.");
			}
			wallet = _createWallet(keyManager);
			try
			{
				keyManager.ToFile();
				WalletDirectories.SetConfiguredWalletName(wallet.WalletName);
				_wallet = wallet;
			}
			catch
			{
				wallet.Dispose();
				throw;
			}
		}

		WalletAdded?.Invoke(this, wallet);
		return wallet;
	}

	public void RenameWallet(Wallet wallet, string newWalletName)
	{
		lock (_lock)
		{
			AssertConfiguredWallet(wallet);
			if (newWalletName == wallet.WalletName)
			{
				return;
			}
			if (ValidateWalletName(newWalletName) is { } error)
			{
				throw new InvalidOperationException($"Invalid name {newWalletName} - {error.Message}");
			}

			var oldPath = WalletDirectories.GetWalletFilePaths(wallet.WalletName + ".json");
			var newPath = WalletDirectories.GetWalletFilePaths(newWalletName + ".json");
			File.Move(oldPath, newPath);
			try
			{
				WalletDirectories.SetConfiguredWalletName(newWalletName);
			}
			catch
			{
				File.Move(newPath, oldPath);
				throw;
			}
			wallet.KeyManager.SetFilePath(newPath);
		}
	}

	public (ErrorSeverity Severity, string Message)? ValidateWalletName(string walletName)
	{
		if (string.IsNullOrEmpty(walletName))
		{
			return (ErrorSeverity.Error, "The name cannot be empty");
		}
		if (walletName.IsTrimmable())
		{
			return (ErrorSeverity.Error, "Leading and trailing white spaces are not allowed!");
		}
		if (!WalletGenerator.ValidateWalletName(walletName))
		{
			return (ErrorSeverity.Error, "Selected wallet name is not valid. Please try a different name.");
		}
		if (File.Exists(WalletDirectories.GetWalletFilePaths(walletName + ".json")))
		{
			return (ErrorSeverity.Error, $"A wallet named {walletName} already exists. Please try a different name.");
		}
		return null;
	}

	public async Task<Wallet> StartWalletAsync(Wallet wallet)
	{
		using (await _startStopWalletLock.LockAsync(_cancelTasksToken).ConfigureAwait(false))
		{
			lock (_lock)
			{
				AssertConfiguredWallet(wallet);
				_cancelTasksToken.ThrowIfCancellationRequested();
			}
			if (wallet.Loaded)
			{
				return wallet;
			}
			try
			{
				Logger.LogInfo(FormatLog("Starting wallet...", wallet));
				await wallet.StartAsync(_cancelTasksToken).ConfigureAwait(false);
				_cancelTasksToken.ThrowIfCancellationRequested();
				Logger.LogInfo(FormatLog("Wallet started.", wallet));
				return wallet;
			}
			catch
			{
				await wallet.StopAsync(CancellationToken.None).ConfigureAwait(false);
				throw;
			}
		}
	}

	public async Task RemoveAndStopAsync(CancellationToken cancel)
	{
		lock (_lock)
		{
			if (_disposed)
			{
				return;
			}
			_disposed = true;
		}
		_cancelTasks.Cancel();
		using (await _startStopWalletLock.LockAsync(cancel).ConfigureAwait(false))
		{
			Wallet? wallet;
			lock (_lock)
			{
				wallet = _wallet;
				_wallet = null;
			}
			if (wallet is not null)
			{
				try
				{
					if (wallet.Loaded)
					{
						await wallet.StopAsync(cancel).ConfigureAwait(false);
						Logger.LogInfo(FormatLog("Wallet stopped.", wallet));
					}
				}
				finally
				{
					wallet.Dispose();
				}
			}
		}
		_cancelTasks.Dispose();
	}

	public void SetMaxBestHeight(uint bestHeight)
	{
		if (GetWallet() is { } wallet && wallet.KeyManager.GetNetwork() == Network)
		{
			wallet.KeyManager.SetMaxBestHeight(bestHeight);
		}
	}

	public ChainHeight? GetBirthHeight() => GetWallet() is { } wallet && wallet.KeyManager.GetNetwork() == Network && wallet.KeyManager.GetBirthHeight() is { } height && height > 0 ? height : null;
	public ChainHeight? GetBestHeight() => GetWallet() is { } wallet && wallet.KeyManager.GetNetwork() == Network ? wallet.KeyManager.GetBestHeight() : null;

	private void AssertConfiguredWallet(Wallet wallet)
	{
		ObjectDisposedException.ThrowIf(_disposed, this);
		if (!ReferenceEquals(_wallet, wallet))
		{
			throw new InvalidOperationException("This is not the configured wallet.");
		}
	}
}
