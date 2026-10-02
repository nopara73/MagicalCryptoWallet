using Microsoft.Extensions.Hosting;
using System.Threading;
using MagicalCryptoWallet.Exceptions;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.WabiSabi.Client.Banning;
using MagicalCryptoWallet.WabiSabi.Client.Batching;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoinProgressEvents;
using MagicalCryptoWallet.WabiSabi.Client.RoundStateAwaiters;
using MagicalCryptoWallet.WabiSabi.Client.StatusChangedEvents;
using MagicalCryptoWallet.WabiSabi.Coordinator.PostRequests;
using static MagicalCryptoWallet.Logging.LoggerTools;

namespace MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;

/// <summary>One actor owns CoinJoin orchestration; protocol round and participant state stays in the client.</summary>
public class CoinJoinManager : BackgroundService
{
	private readonly WalletSession _session;
	private readonly RoundStateProvider _roundStatusProvider;
	private readonly CoinPrison _coinPrison;
	private readonly CoinRefrigerator _coinRefrigerator = new();
	private readonly CoinJoinConfiguration _coinJoinConfiguration;
	private readonly Func<string, IWabiSabiApiRequestHandler> ArenaRequestHandlerFactory;
	private readonly MailboxProcessor<CoinJoinCommand> _mailboxProcessor;
	private readonly CancellationTokenSource _stopCts = new();
	private readonly IDisposable _serverTipHeightChangeSubscription;
	private readonly IDisposable _recoveryRegistration;
	private CoinJoinTracker? _tracker;
	private readonly Lock _snapshotGate = new();
	private CoinJoinSnapshot _snapshot = new(CoinJoinClientState.Idle, ImmutableList<SmartCoin>.Empty, false, false, false, false, false, null, default);
	private event EventHandler<CoinJoinSnapshot>? SnapshotChanged;
	private Task? _stopTask;
	private Task? _observerTask;
	private bool _started;
	private StatusChangedEventArgs? _lastStatus;
	private int _sendHolds;
	private bool _shutdownHold;
	private bool _paused;
	private bool _overridePlebStop;
	private uint _serverTipHeight;
	private readonly TimeProvider _timeProvider;
	private DateTimeOffset _retryAfter;
	private CoinjoinError? _waitingReason;
	private static readonly TimeSpan FailureBackoff = TimeSpan.FromSeconds(30);

	public CoinJoinManager(WalletSession session, RoundStateProvider roundStatusProvider,
		Func<string, IWabiSabiApiRequestHandler> arenaRequestHandlerFactory, CoinJoinConfiguration coinJoinConfiguration,
		CoinPrison coinPrison, EventBus eventBus, TimeProvider? timeProvider = null)
	{
		_session = session;
		_timeProvider = timeProvider ?? TimeProvider.System;
		_roundStatusProvider = roundStatusProvider;
		ArenaRequestHandlerFactory = arenaRequestHandlerFactory;
		_coinJoinConfiguration = coinJoinConfiguration;
		_coinPrison = coinPrison;
		_mailboxProcessor = new MailboxProcessor<CoinJoinCommand>(nameof(CoinJoinManager), HandleCommandsAsync, cancellationToken: _stopCts.Token);
		_serverTipHeightChangeSubscription = eventBus.Subscribe<NetworkTipHeightChanged>(h => _mailboxProcessor.Post(new TipHeightCommand(h.Height)));
		_recoveryRegistration = session.RegisterRecoveryGuard(QuiesceForRecoveryAsync);
		session.OperationAuthorized += OnOperationAuthorized;
	}
	private void OnOperationAuthorized(object? sender, EventArgs args) => _mailboxProcessor.Post(new AuthorizationCommand());
	public event EventHandler<StatusChangedEventArgs>? StatusChanged;
	public CoinJoinSnapshot Snapshot => Volatile.Read(ref _snapshot);
	public bool IsStarted => _started;
	public IDisposable Subscribe(Action<CoinJoinSnapshot> observer)
	{
		void OnChanged(object? sender, CoinJoinSnapshot status) => observer(status);
		lock (_snapshotGate) { SnapshotChanged += OnChanged; observer(_snapshot); }
		return new Subscription(() => { lock (_snapshotGate) { SnapshotChanged -= OnChanged; } });
	}
	public IDisposable SubscribeStatus(Action<StatusChangedEventArgs> observer)
	{
		void OnChanged(object? sender, StatusChangedEventArgs status) => observer(status);
		lock (_snapshotGate)
		{
			StatusChanged += OnChanged;
			observer(ClientState == CoinJoinClientState.Idle ? new WalletStoppedCoinJoinEventArgs() : new WalletStartedCoinJoinEventArgs());
			if (_waitingReason is { } reason) { observer(new StartErrorEventArgs(reason)); }
			else if (_lastStatus is CoinJoinStatusEventArgs current && ClientState is CoinJoinClientState.InProgress or CoinJoinClientState.InCriticalPhase) { observer(current); }
		}
		return new Subscription(() => { lock (_snapshotGate) { StatusChanged -= OnChanged; } });
	}
	private sealed class Subscription(Action dispose) : IDisposable
	{
		private Action? _dispose = dispose;
		public void Dispose() => Interlocked.Exchange(ref _dispose, null)?.Invoke();
	}
	private void Notify(StatusChangedEventArgs args)
	{
		lock (_snapshotGate) { _lastStatus = args; StatusChanged.SafeInvoke(this, args); }
	}
	public ImmutableList<SmartCoin> CoinsInCriticalPhase => Volatile.Read(ref _snapshot).CriticalCoins;
	public CoinJoinClientState ClientState => Volatile.Read(ref _snapshot).State;
	public void RequestCoinJoinStart(bool overridePlebStop = false)
	{
		_session.EnsureReady();
		if (_session.CoinJoinKeyChain is null) { throw new InvalidOperationException("Authorize CoinJoin for this application run first."); }
		_mailboxProcessor.Post(new StartCommand(overridePlebStop));
	}
	public void RequestCoinJoinStop() => _mailboxProcessor.Post(new StopCommand());
	public void WalletEnteredSendWorkflow() => _mailboxProcessor.Post(new EnterSendCommand());
	public void WalletLeftSendWorkflow() => _mailboxProcessor.Post(new LeaveSendCommand());
	public async Task WalletEnteredSendingAsync()
	{
		await PostAndWait(new BeginSendingCommand(new(TaskCreationOptions.RunContinuationsAsynchronously))).ConfigureAwait(false);
		await Task.WaitForAsync(() => Snapshot.State == CoinJoinClientState.Idle, _stopCts.Token).ConfigureAwait(false);
	}
	public Task SignalToStopCoinjoinsAsync() => PostAndWait(new ShutdownCommand(new(TaskCreationOptions.RunContinuationsAsynchronously)));
	public Task RestartAbortedCoinjoinsAsync() => PostAndWait(new ResumeCommand(new(TaskCreationOptions.RunContinuationsAsynchronously)));
	private Task PostAndWait(AwaitableCommand command)
	{
		if (_stopCts.IsCancellationRequested || !_mailboxProcessor.Post(command))
		{
			throw new InvalidOperationException("CoinJoin is stopping.");
		}
		return command.Completion.Task.WaitAsync(_stopCts.Token);
	}
	private static bool IsUnderPlebStop(SmartCoin[] coins, Money threshold) => coins.Sum(x => x.Amount) < threshold;

	public override Task StartAsync(CancellationToken cancellationToken)
	{
		if (_started) { return Task.CompletedTask; }
		_started = true;
		_mailboxProcessor.Start();
		return base.StartAsync(cancellationToken);
	}
	protected override async Task ExecuteAsync(CancellationToken stoppingToken)
	{
		try
		{
			while (!stoppingToken.IsCancellationRequested)
			{
				_mailboxProcessor.Post(new TickCommand());
				await Task.Delay(1_000, stoppingToken).ConfigureAwait(false);
			}
		}
		catch (OperationCanceledException) when (stoppingToken.IsCancellationRequested) { }
	}
	private async Task HandleCommandsAsync(Mailbox<CoinJoinCommand> mailbox, CancellationToken cancel)
	{
		var factory = new CoinJoinTrackerFactory(ArenaRequestHandlerFactory, _roundStatusProvider, _coinJoinConfiguration, cancel);
		try
		{
			while (!cancel.IsCancellationRequested)
			{
				var command = await mailbox.ReceiveAsync(cancel).ConfigureAwait(false);
				try
				{
					switch (command)
					{
						case StartCommand start:
							_paused = false;
							_overridePlebStop = start.OverridePlebStop;
							_retryAfter = default;
							_waitingReason = null;
							break;
						case StopCommand:
							_paused = true;
							_overridePlebStop = false;
							_waitingReason = null;
							StopCore();
							break;
						case FinishedCommand finished when ReferenceEquals(_tracker, finished.Tracker):
							await HandleCoinJoinFinalizationAsync(finished.Tracker, cancel).ConfigureAwait(false); break;
						case ProgressCommand progress when ReferenceEquals(_tracker, progress.Tracker): NotifyCoinJoinStatusChanged(progress.Args); break;
						case EnterSendCommand: _sendHolds++; break;
						case BeginSendingCommand: StopCore(); break;
						case LeaveSendCommand: _sendHolds = Math.Max(0, _sendHolds - 1); break;
						case ShutdownCommand: _shutdownHold = true; StopCore(); break;
						case ResumeCommand: _shutdownHold = false; break;
						case ClearPlebOverrideCommand: _overridePlebStop = false; break;
						case TipHeightCommand tip: _serverTipHeight = tip.Height; break;
					}
					Reconcile(factory, cancel);
					UpdateSnapshot();
					if (command is AwaitableCommand awaited) { awaited.Completion.TrySetResult(); }
				}
				catch (Exception ex)
				{
					if (command is AwaitableCommand awaited) { awaited.Completion.TrySetException(ex); }
					Logger.LogError(ex);
				}
			}
		}
		catch (OperationCanceledException) when (cancel.IsCancellationRequested) { }
		finally
		{
			_shutdownHold = true;
			if (_tracker is { } tracker)
			{
				tracker.Stop();
				try { await tracker.CoinJoinTask.ConfigureAwait(false); } catch (Exception ex) { Logger.LogDebug(ex); }
				await HandleCoinJoinFinalizationAsync(tracker, cancel).ConfigureAwait(false);
			}
			UpdateSnapshot();
		}
	}
	private void Reconcile(CoinJoinTrackerFactory factory, CancellationToken cancel)
	{
		if (cancel.IsCancellationRequested || _paused) { return; }
		if (_tracker is { InputRegistrationStarted: false } waitingTracker &&
			(_shutdownHold || _sendHolds > 0 || !_session.Snapshot.IsSynchronized ||
			_session.GetWallet() is { } completedWallet && completedWallet.IsWalletPrivate() && !completedWallet.BatchedPayments.AreTherePendingPayments))
		{
			waitingTracker.Stop();
			return;
		}
		if (_tracker is not null) { return; }
		if (_shutdownHold || _sendHolds > 0 || !_session.Snapshot.IsSynchronized || _session.CoinJoinKeyChain is null || _session.GetWallet() is not { } wallet)
		{
			_waitingReason = null;
			return;
		}
		var candidates = GetCoinSelection(wallet);
		if (GetReadinessError(candidates, wallet, _overridePlebStop) is { } reason)
		{
			_waitingReason = reason;
			return;
		}
		if (_timeProvider.GetUtcNow() < _retryAfter) { return; }
		_retryAfter = default;
		_waitingReason = null;
		if (!IsUnderPlebStop(candidates.CandidateCoins, wallet.PlebStopThreshold)) { _overridePlebStop = false; }
		UpdateSnapshot();
		StartCore(wallet, factory);
	}

	private static CoinjoinError? GetReadinessError(CoinSelectionResult result, Wallet wallet, bool overridePlebStop)
	{
		if (!wallet.BatchedPayments.AreTherePendingPayments && wallet.IsWalletPrivate()) { return CoinjoinError.AllCoinsPrivate; }
		var coins = result.CandidateCoins;
		if (coins.Length == 0 || !wallet.BatchedPayments.AreTherePendingPayments && coins.All(x => x.IsPrivate(Constants.AnonymityScoreTarget)))
		{
			return GetUnavailableNonPrivateCoinsError(result, wallet);
		}
		if (!overridePlebStop && IsUnderPlebStop(coins, wallet.PlebStopThreshold))
		{
			return IsUnderPlebStop(coins.Concat(result.UnconfirmedCoins).ToArray(), wallet.PlebStopThreshold)
				? CoinjoinError.NotEnoughUnprivateBalance
				: CoinjoinError.NotEnoughConfirmedUnprivateBalance;
		}
		return null;
	}

	private void StartCore(Wallet wallet, CoinJoinTrackerFactory factory)
	{
		var keyChain = _session.CoinJoinKeyChain ?? throw new InvalidOperationException("CoinJoin requires authorization.");
		IEnumerable<SmartCoin> GetCoinCandidates()
		{
			_session.EnsureReady();
			if (Snapshot.SendRestricted || Snapshot.ShutdownRestricted) { throw new CoinJoinClientException(CoinjoinError.UserInSendWorkflow); }
			var selection = GetCoinSelection(wallet);
			if (GetReadinessError(selection, wallet, Snapshot.OverridePlebStop) is { } error) { throw new CoinJoinClientException(error); }
			if (!IsUnderPlebStop(selection.CandidateCoins, wallet.PlebStopThreshold)) { _mailboxProcessor.Post(new ClearPlebOverrideCommand()); }
			return selection.CandidateCoins;
		}
		_tracker = factory.CreateAndStart(wallet, keyChain, GetCoinCandidates, _overridePlebStop);
		_tracker.WalletCoinJoinProgressChanged += CoinJoinTracker_WalletCoinJoinProgressChanged;
		NotifyCoinJoinStarted(TimeSpan.MaxValue);
		NotifyCoinJoinStatusChanged(_tracker.CurrentProgress ?? new WaitingForRound());
		_observerTask = ObserveCompletionAsync(_tracker);
	}
	private async Task ObserveCompletionAsync(CoinJoinTracker tracker)
	{
		try { await tracker.CoinJoinTask.ConfigureAwait(false); } catch (Exception) { /* The actor reconciles every outcome. */ }
		if (!_stopCts.IsCancellationRequested) { _mailboxProcessor.Post(new FinishedCommand(tracker)); }
	}
	private void StopCore()
	{
		if (_tracker is { } tracker) { tracker.Stop(); }
		UpdateSnapshot();
	}
	private record CoinSelectionResult(SmartCoin[] CandidateCoins, SmartCoin[] BannedCoins, SmartCoin[] ImmatureCoins, SmartCoin[] UnconfirmedCoins)
	{
		public CoinSelectionResult() : this([], [], [], []) { }
	}

	private CoinSelectionResult GetCoinSelection(Wallet wallet)
	{
		var coinCandidates = new CoinsView(wallet.GetCoinjoinCoinCandidates())
			.Available()
			.Where(x => !_coinRefrigerator.IsFrozen(x))
			.ToArray();

		if (coinCandidates.Length == 0)
		{
			return new CoinSelectionResult();
		}

		var bannedCoins = coinCandidates.Where(x => _coinPrison.IsBanned(x.Outpoint)).ToArray();
		var immatureCoins = _serverTipHeight > 0
			? coinCandidates.Where(x => x.Transaction.IsImmature(_serverTipHeight)).ToArray()
			: [];
		var unconfirmedCoins = coinCandidates.Where(x => !x.Confirmed).ToArray();

		var availableCoins = coinCandidates
			.Except(bannedCoins)
			.Except(immatureCoins)
			.Except(unconfirmedCoins)
			.ToArray();

		return new CoinSelectionResult(
			availableCoins,
			bannedCoins,
			immatureCoins,
			unconfirmedCoins);
	}

	private static CoinjoinError GetUnavailableNonPrivateCoinsError(CoinSelectionResult result, Wallet wallet)
	{
		bool AnyNonPrivate(SmartCoin[] coins) => coins.Any(x => wallet.BatchedPayments.AreTherePendingPayments || !x.IsPrivate(Constants.AnonymityScoreTarget));

		if (AnyNonPrivate(result.UnconfirmedCoins))
		{
			return CoinjoinError.NoConfirmedCoinsEligibleToMix;
		}

		if (AnyNonPrivate(result.ImmatureCoins))
		{
			return CoinjoinError.OnlyImmatureCoinsAvailable;
		}

		if (AnyNonPrivate(result.BannedCoins))
		{
			return CoinjoinError.CoinsRejected;
		}

		return CoinjoinError.NoCoinsEligibleToMix;
	}

	private void UpdateSnapshot()
	{
		var waiting = !_paused && !_shutdownHold && _sendHolds == 0 && _session.Snapshot.IsSynchronized && _session.CoinJoinKeyChain is not null && _waitingReason != CoinjoinError.AllCoinsPrivate;
		var state = _tracker is { } tracker ? tracker.InCriticalCoinJoinState ? CoinJoinClientState.InCriticalPhase : CoinJoinClientState.InProgress
			: waiting ? CoinJoinClientState.InSchedule : CoinJoinClientState.Idle;
		var snapshot = new CoinJoinSnapshot(state, _tracker?.CoinsInCriticalPhase ?? ImmutableList<SmartCoin>.Empty, _paused, _overridePlebStop,
			_tracker?.InputRegistrationStarted ?? false, _sendHolds > 0, _shutdownHold, _waitingReason, _retryAfter);
		lock (_snapshotGate)
		{
			var previous = _snapshot;
			if (previous == snapshot) { return; }
			Volatile.Write(ref _snapshot, snapshot);
			SnapshotChanged.SafeInvoke(this, snapshot);
			if (previous.State == CoinJoinClientState.Idle && state != CoinJoinClientState.Idle) { NotifyWalletStartedCoinJoin(); }
			else if (previous.State != CoinJoinClientState.Idle && state == CoinJoinClientState.Idle) { NotifyWalletStoppedCoinJoin(); }
			if (previous.WaitingReason != _waitingReason && _waitingReason is { } reason) { NotifyCoinJoinStartError(reason); }
		}
	}

	private async Task HandleCoinJoinFinalizationAsync(CoinJoinTracker finishedCoinJoin, CancellationToken cancellationToken)
	{
		var wallet = finishedCoinJoin.Wallet;
		var destinationProvider = wallet.OutputProvider.DestinationProvider;
		var batchedPayments = wallet.BatchedPayments;
		CoinJoinClientException? cjClientException = null;
		var forceStop = false;
		var unknownEnding = false;
		var retryFailure = false;
		try
		{
			var result = await finishedCoinJoin.CoinJoinTask.ConfigureAwait(false);
			if (result is SuccessfulCoinJoinResult successfulCoinjoin)
			{
				var coinjoinTxId = successfulCoinjoin.UnsignedCoinJoin.GetHash();
				var paymentsTotal = Money.Satoshis(batchedPayments.GetPayments()
					.Where(p => p.State switch
					{
						SignedUnknownPayment signed => signed.TransactionId == coinjoinTxId,
						FinishedPayment finished => finished.TransactionId == coinjoinTxId,
						_ => false
					})
					.Sum(p => p.Amount));
				_coinRefrigerator.Freeze(successfulCoinjoin.Coins);
				batchedPayments.MovePaymentsToFinished(coinjoinTxId);
				MarkDestinationsUsed(destinationProvider, successfulCoinjoin.OutputScripts);
				wallet.KeyManager.AddCoinjoinCosts(coinjoinTxId, successfulCoinjoin.Costs with { PaymentsTotal = paymentsTotal });
				Logger.LogInfo(FormatLog($"{nameof(CoinJoinClient)} finished. Coinjoin transaction was broadcast.", wallet));
			}
			else
			{
				retryFailure = true;
				Logger.LogInfo(FormatLog($"{nameof(CoinJoinClient)} finished. Coinjoin transaction was not broadcast.", wallet));
			}
		}
		catch (UnknownRoundEndingException ex)
		{
			// The round ending is unknown - the transaction might have been broadcast.
			// Payments are already in signed state (moved by TransactionSigned event).
			// The reconciliation process will later check if the transaction was confirmed.
			unknownEnding = true;
			retryFailure = true;
			_coinRefrigerator.Freeze(ex.Coins);
			MarkDestinationsUsed(destinationProvider, ex.OutputScripts);
			Logger.LogWarning(FormatLog($"Round ending unknown - payments in signed state awaiting resolution: {ex.Message}", wallet));
		}
		catch (CoinJoinClientException clientException)
		{
			cjClientException = clientException;
			// Round-dependent eligibility can change with the next round. Back off rather than
			// recreating a tracker in a tight loop when selection or registration fails.
			retryFailure = true;
			if (cjClientException.CoinjoinError is CoinjoinError.CoordinatorLiedAboutInputs)
			{
				Logger.LogError(cjClientException);
				forceStop = true;
			}
			else
			{
				Logger.LogDebug(cjClientException);
			}
		}
		catch (InvalidOperationException ioe)
		{
			retryFailure = true;
			Logger.LogWarning(ioe);
		}
		catch (OperationCanceledException)
		{
			if (finishedCoinJoin.IsStopped)
			{
				Logger.LogInfo($"{nameof(CoinJoinClient)} stopped.", wallet);
			}
			else
			{
				retryFailure = true;
				Logger.LogInfo($"{nameof(CoinJoinClient)} was cancelled.", wallet);
			}
		}
		catch (UnexpectedRoundPhaseException e)
		{
			retryFailure = true;
			// `UnexpectedRoundPhaseException` indicates an error in the protocol however,
			// temporarily we are shortening the circuit by aborting the rounds if
			// there are Alices that didn't confirm.
			// The fix is already done but the clients have to upgrade.
			Logger.LogInfo(FormatLog($"{nameof(CoinJoinClient)} failed with exception: '{e}'", wallet));
		}
		catch (WabiSabiProtocolException wpe) when (wpe.ErrorCode == WabiSabiProtocolErrorCode.WrongPhase)
		{
			retryFailure = true;
			// This can happen when the coordinator aborts the round in Signing phase because of detected double spend.
			Logger.LogInfo(FormatLog($"{nameof(CoinJoinClient)} failed with: '{wpe.Message}'", wallet));
		}
		catch (Exception e)
		{
			retryFailure = true;
			Logger.LogError(FormatLog($"{nameof(CoinJoinClient)} failed with exception: '{e}'", wallet));
		}
		finally
		{
			// Only move payments to pending if we know the round failed.
			if (!unknownEnding)
			{
				batchedPayments.MovePaymentsToPending();
			}
		}

		// If any coins were marked for banning, store them to file
		if (finishedCoinJoin.BannedCoins.Count != 0)
		{
			foreach (var info in finishedCoinJoin.BannedCoins)
			{
				_coinPrison.Ban(info.Coin, info.BanUntilUtc);
			}
		}

		NotifyCoinJoinCompletion(finishedCoinJoin);

		if (forceStop) { _paused = true; }
		if (retryFailure && !finishedCoinJoin.IsStopped && !cancellationToken.IsCancellationRequested)
		{
			_retryAfter = _timeProvider.GetUtcNow() + FailureBackoff;
		}
		_waitingReason = cjClientException?.CoinjoinError;
		finishedCoinJoin.WalletCoinJoinProgressChanged -= CoinJoinTracker_WalletCoinJoinProgressChanged;
		finishedCoinJoin.Dispose();
		_tracker = null;
	}

	private static void MarkDestinationsUsed(IDestinationProvider provider, ImmutableList<Script> outputs) => provider.TrySetScriptStates(KeyState.Used, outputs);
	private void NotifyWalletStartedCoinJoin() => Notify(new WalletStartedCoinJoinEventArgs());
	private void NotifyWalletStoppedCoinJoin() => Notify(new WalletStoppedCoinJoinEventArgs());
	private void NotifyCoinJoinStarted(TimeSpan timeout) => Notify(new StartedEventArgs(timeout));
	private void NotifyCoinJoinStartError(CoinjoinError error) => Notify(new StartErrorEventArgs(error));
	private void NotifyCoinJoinCompletion(CoinJoinTracker tracker)
	{
		var status = tracker.CoinJoinTask.Status switch
		{
			TaskStatus.RanToCompletion when tracker.CoinJoinTask.Result is SuccessfulCoinJoinResult => CompletionStatus.Success,
			TaskStatus.Canceled => CompletionStatus.Canceled, TaskStatus.Faulted => CompletionStatus.Failed, _ => CompletionStatus.Unknown
		};
		Notify(new CompletedEventArgs(status));
	}
	private void NotifyCoinJoinStatusChanged(CoinJoinProgressEventArgs args)
	{
		Notify(new CoinJoinStatusEventArgs(args));
	}
	private void CoinJoinTracker_WalletCoinJoinProgressChanged(object? sender, CoinJoinProgressEventArgs e) { if (sender is CoinJoinTracker tracker) { _mailboxProcessor.Post(new ProgressCommand(tracker, e)); } }
	public override Task StopAsync(CancellationToken cancellationToken)
	{
		lock (_snapshotGate) { return _stopTask ??= StopServiceAsync(cancellationToken); }
	}
	private async Task StopServiceAsync(CancellationToken cancellationToken)
	{
		if (_started && !_stopCts.IsCancellationRequested)
		{
			await SignalToStopCoinjoinsAsync().WaitAsync(cancellationToken).ConfigureAwait(false);
			await Task.WaitForAsync(() => ClientState != CoinJoinClientState.InCriticalPhase, cancellationToken).ConfigureAwait(false);
		}
		await _stopCts.CancelAsync().ConfigureAwait(false);
		await base.StopAsync(cancellationToken).ConfigureAwait(false);
		await _mailboxProcessor.Completion.WaitAsync(cancellationToken).ConfigureAwait(false);
		if (_observerTask is { } observer) { await observer.WaitAsync(cancellationToken).ConfigureAwait(false); }
	}
	public override void Dispose()
	{
		_session.OperationAuthorized -= OnOperationAuthorized;
		_recoveryRegistration.Dispose(); _mailboxProcessor.Dispose(); _stopCts.Dispose(); _serverTipHeightChangeSubscription.Dispose(); base.Dispose();
	}
	private async Task<IAsyncDisposable> QuiesceForRecoveryAsync(CancellationToken cancel)
	{
		if (!_started) { return new RecoveryGuard(this, false); }
		await SignalToStopCoinjoinsAsync().WaitAsync(cancel).ConfigureAwait(false);
		try { await Task.WaitForAsync(() => Snapshot.State == CoinJoinClientState.Idle, cancel).ConfigureAwait(false); }
		catch { await RestartAbortedCoinjoinsAsync().ConfigureAwait(false); throw; }
		return new RecoveryGuard(this, true);
	}
	private sealed class RecoveryGuard(CoinJoinManager manager, bool held) : IAsyncDisposable
	{
		public async ValueTask DisposeAsync()
		{
			if (held && !manager._stopCts.IsCancellationRequested) { await manager.RestartAbortedCoinjoinsAsync().ConfigureAwait(false); }
		}
	}
	private abstract record CoinJoinCommand;
	private record TipHeightCommand(uint Height) : CoinJoinCommand;
	private record AuthorizationCommand : CoinJoinCommand;
	private record StartCommand(bool OverridePlebStop) : CoinJoinCommand;
	private record StopCommand : CoinJoinCommand;
	private record ClearPlebOverrideCommand : CoinJoinCommand;
	private record TickCommand : CoinJoinCommand;
	private record FinishedCommand(CoinJoinTracker Tracker) : CoinJoinCommand;
	private record ProgressCommand(CoinJoinTracker Tracker, CoinJoinProgressEventArgs Args) : CoinJoinCommand;
	private record EnterSendCommand : CoinJoinCommand;
	private record LeaveSendCommand : CoinJoinCommand;
	private abstract record AwaitableCommand(TaskCompletionSource Completion) : CoinJoinCommand;
	private record BeginSendingCommand(TaskCompletionSource Completion) : AwaitableCommand(Completion);
	private record ShutdownCommand(TaskCompletionSource Completion) : AwaitableCommand(Completion);
	private record ResumeCommand(TaskCompletionSource Completion) : AwaitableCommand(Completion);
}

public record CoinJoinConfiguration(string CoordinatorIdentifier, decimal MaxCoinJoinMiningFeeRate);

public record CoinJoinSnapshot(CoinJoinClientState State, ImmutableList<SmartCoin> CriticalCoins, bool IsPaused, bool OverridePlebStop, bool IsRunning, bool SendRestricted, bool ShutdownRestricted, CoinjoinError? WaitingReason, DateTimeOffset RetryAfter);
