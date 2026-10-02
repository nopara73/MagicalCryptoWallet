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
	private readonly InputVerifier _inputVerifier;
	private readonly CoinRefrigerator _coinRefrigerator = new();
	private readonly CoinJoinConfiguration _coinJoinConfiguration;
	private readonly Func<string, IWabiSabiApiRequestHandler> ArenaRequestHandlerFactory;
	private readonly MailboxProcessor<CoinJoinCommand> _mailboxProcessor;
	private readonly CancellationTokenSource _stopCts = new();
	private readonly IDisposable _serverTipHeightChangeSubscription;
	private readonly IDisposable _recoveryRegistration;
	private bool _resumeWhenReady;
	private CoinJoinTracker? _tracker;
	private PendingRestart? _restart;
	private readonly Lock _snapshotGate = new();
	private CoinJoinSnapshot _snapshot = new(CoinJoinClientState.Idle, [], true, false, false, false, false);
	private event EventHandler<CoinJoinSnapshot>? SnapshotChanged;
	private Task? _stopTask;
	private Task? _observerTask;
	private bool _startRequested;
	private bool _started;
	private bool? _previousAutoSetting;
	private StatusChangedEventArgs? _lastStatus;
	private int _sendHolds;
	private bool _shutdownHold;
	private bool _resumeAfterSend;
	private bool _resumeAfterShutdown;
	private bool _paused;
	private bool _stopWhenAllMixed = true;
	private bool _overridePlebStop;
	private bool _automaticStartConsidered;
	private uint _serverTipHeight;

	public CoinJoinManager(WalletSession session, RoundStateProvider roundStatusProvider,
		Func<string, IWabiSabiApiRequestHandler> arenaRequestHandlerFactory, CoinJoinConfiguration coinJoinConfiguration,
		CoinPrison coinPrison, InputVerifier inputVerifier, EventBus eventBus)
	{
		_session = session;
		_roundStatusProvider = roundStatusProvider;
		ArenaRequestHandlerFactory = arenaRequestHandlerFactory;
		_coinJoinConfiguration = coinJoinConfiguration;
		_coinPrison = coinPrison;
		_inputVerifier = inputVerifier;
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
			if (_lastStatus is CoinJoinStatusEventArgs current && ClientState is CoinJoinClientState.InProgress or CoinJoinClientState.InCriticalPhase) { observer(current); }
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
	public void RequestCoinJoinStart(bool stopWhenAllMixed, bool overridePlebStop)
	{
		_session.EnsureReady();
		if (_session.CoinJoinKeyChain is null) { throw new InvalidOperationException("Authorize CoinJoin for this application run first."); }
		_mailboxProcessor.Post(new StartCommand(stopWhenAllMixed, overridePlebStop));
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
		var factory = new CoinJoinTrackerFactory(ArenaRequestHandlerFactory, _roundStatusProvider, _coinJoinConfiguration, _inputVerifier, cancel);
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
							_stopWhenAllMixed = start.StopWhenAllMixed;
							_overridePlebStop = start.OverridePlebStop;
							await CancelRestartAsync().ConfigureAwait(false);
							StartCore(start, factory);
							break;
						case StopCommand: _paused = true; _startRequested = _resumeWhenReady = false; await CancelRestartAsync().ConfigureAwait(false); StopCore(); break;
						case RestartCommand restart when _restart?.Id == restart.Id:
							var settings = _restart;
							await CancelRestartAsync().ConfigureAwait(false);
							if (!_paused && !_shutdownHold && _sendHolds == 0) { StartCore(new(settings.StopWhenAllMixed, settings.OverridePlebStop), factory); }
							break;
						case FinishedCommand finished when ReferenceEquals(_tracker, finished.Tracker):
							await HandleCoinJoinFinalizationAsync(finished.Tracker, cancel).ConfigureAwait(false); break;
						case ProgressCommand progress when ReferenceEquals(_tracker, progress.Tracker): NotifyCoinJoinStatusChanged(progress.Args); break;
						case EnterSendCommand: _resumeAfterSend |= ClientState != CoinJoinClientState.Idle; _sendHolds++; break;
						case BeginSendingCommand:
							_resumeAfterSend |= ClientState != CoinJoinClientState.Idle;
							await CancelRestartAsync().ConfigureAwait(false); StopCore(); break;
						case LeaveSendCommand:
							_sendHolds = Math.Max(0, _sendHolds - 1);
							if (_sendHolds == 0 && !_shutdownHold && (_resumeAfterSend || _resumeAfterShutdown) && !_paused)
							{ _resumeAfterSend = _resumeAfterShutdown = false; await ScheduleRestartAutomaticallyAsync(_stopWhenAllMixed, _overridePlebStop, cancel).ConfigureAwait(false); }
							break;
						case ShutdownCommand:
							_resumeAfterShutdown |= ClientState != CoinJoinClientState.Idle;
							_shutdownHold = true; await CancelRestartAsync().ConfigureAwait(false); StopCore(); break;
						case ResumeCommand:
							_shutdownHold = false;
							if (_sendHolds == 0 && (_resumeAfterSend || _resumeAfterShutdown) && !_paused)
							{ _resumeAfterSend = _resumeAfterShutdown = false; await ScheduleRestartAutomaticallyAsync(_stopWhenAllMixed, _overridePlebStop, cancel).ConfigureAwait(false); }
							break;
						case ClearPlebOverrideCommand: _overridePlebStop = false; break;
						case TipHeightCommand tip: _serverTipHeight = tip.Height; break;
						case AuthorizationCommand:
							if (_session.GetWallet()?.KeyManager.AutoCoinJoin == true && !_paused && _tracker is null && _restart is null)
							{
								_automaticStartConsidered = true;
								_stopWhenAllMixed = false;
								_resumeWhenReady = true;
								await ConsiderAutomaticStartAsync(cancel).ConfigureAwait(false);
							}
							break;
						case TickCommand:
							await ConsiderAutomaticStartAsync(cancel).ConfigureAwait(false); break;
					}
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
			await CancelRestartAsync().ConfigureAwait(false);
			if (_tracker is { } tracker)
			{
				tracker.Stop();
				try { await tracker.CoinJoinTask.ConfigureAwait(false); } catch (Exception ex) { Logger.LogDebug(ex); }
				await HandleCoinJoinFinalizationAsync(tracker, cancel).ConfigureAwait(false);
			}
		}
	}
	private async Task ConsiderAutomaticStartAsync(CancellationToken cancel)
	{
		if (_session.GetWallet() is not { } wallet) { return; }
		if (_previousAutoSetting is { } previous && previous != wallet.KeyManager.AutoCoinJoin)
		{
			_automaticStartConsidered = false;
			if (wallet.KeyManager.AutoCoinJoin) { _paused = false; }
			else { _paused = true; _resumeWhenReady = false; await CancelRestartAsync().ConfigureAwait(false); StopCore(); }
		}
		_previousAutoSetting = wallet.KeyManager.AutoCoinJoin;
		if (_resumeWhenReady && !_paused && !_shutdownHold && _sendHolds == 0 && _tracker is null && _restart is null && _session.Snapshot.IsSynchronized && _session.CoinJoinKeyChain is not null)
		{
			_resumeWhenReady = false;
			await ScheduleRestartAutomaticallyAsync(_stopWhenAllMixed, _overridePlebStop, cancel, TimeSpan.Zero).ConfigureAwait(false);
		}
		if (!_automaticStartConsidered && _tracker is null && _restart is null && !_paused && !_shutdownHold && _sendHolds == 0 && wallet.KeyManager.AutoCoinJoin && _session.Snapshot.IsSynchronized && _session.CoinJoinKeyChain is not null)
		{
			_automaticStartConsidered = true;
			await ScheduleRestartAutomaticallyAsync(false, false, cancel, TimeSpan.FromSeconds(Random.Shared.Next(60, 180))).ConfigureAwait(false);
		}
	}
	private void StartCore(StartCommand startCommand, CoinJoinTrackerFactory factory)
	{
		if (!_session.Snapshot.IsSynchronized || _session.CoinJoinKeyChain is null) { _resumeWhenReady = true; return; }
		_resumeWhenReady = false;
		var walletToStart = _session.GetWallet() ?? throw new InvalidOperationException("No wallet is configured.");
		var keyChain = _session.CoinJoinKeyChain ?? throw new InvalidOperationException("CoinJoin requires authorization.");
		if (_shutdownHold || _sendHolds > 0) { _resumeAfterSend = true; return; }
		if (_tracker is { } running)
		{
			running.StopWhenAllMixed = startCommand.StopWhenAllMixed;
			if (running.IsStopped) { _startRequested = true; }
			return;
		}
		IEnumerable<SmartCoin> SanityChecksAndGetCoinCandidatesFunc()
		{
			if (Snapshot.SendRestricted || Snapshot.ShutdownRestricted)
			{
				throw new CoinJoinClientException(CoinjoinError.UserInSendWorkflow);
			}

			var coinSelectionResult = SelectCandidateCoins(walletToStart);
			var coinCandidates = coinSelectionResult.CandidateCoins;

			if (IsUnderPlebStop(coinCandidates, walletToStart.PlebStopThreshold) && !Snapshot.OverridePlebStop)
			{
				Logger.LogTrace(FormatLog("PlebStop preventing coinjoin.", walletToStart));

				if (!IsUnderPlebStop(coinCandidates.Union(coinSelectionResult.UnconfirmedCoins).ToArray(), walletToStart.PlebStopThreshold))
				{
					throw new CoinJoinClientException(CoinjoinError.NotEnoughConfirmedUnprivateBalance);
				}

				throw new CoinJoinClientException(CoinjoinError.NotEnoughUnprivateBalance);
			}

			// If there are pending payments, ignore already achieved privacy.
			if (!walletToStart.BatchedPayments.AreTherePendingPayments)
			{
				// If all coins are already private, then don't mix.
				if (walletToStart.IsWalletPrivate())
				{
					Logger.LogTrace(FormatLog("All mixed!", walletToStart));
					throw new CoinJoinClientException(CoinjoinError.AllCoinsPrivate);
				}

				// If all coin candidates are private it makes no sense to mix them.
				if (coinCandidates.All(x => x.IsPrivate(walletToStart.AnonScoreTarget)))
				{
					throw new CoinJoinClientException(
						GetUnavailableNonPrivateCoinsError(coinSelectionResult, walletToStart),
						$"All coin candidates are already private and {nameof(startCommand.StopWhenAllMixed)} was {startCommand.StopWhenAllMixed}");
				}
			}

			if (!IsUnderPlebStop(coinCandidates, walletToStart.PlebStopThreshold)) { _mailboxProcessor.Post(new ClearPlebOverrideCommand()); }

			return coinCandidates;
		}

		UpdateSnapshot();
		_tracker = factory.CreateAndStart(walletToStart, keyChain, SanityChecksAndGetCoinCandidatesFunc, startCommand.StopWhenAllMixed, startCommand.OverridePlebStop);
		_tracker.WalletCoinJoinProgressChanged += CoinJoinTracker_WalletCoinJoinProgressChanged;
		NotifyCoinJoinStarted(TimeSpan.MaxValue);
		_observerTask = ObserveCompletionAsync(_tracker);
		NotifyWalletStartedCoinJoin();
		UpdateSnapshot();
	}
	private async Task ObserveCompletionAsync(CoinJoinTracker tracker)
	{
		try { await tracker.CoinJoinTask.ConfigureAwait(false); } catch (Exception) { /* The actor reconciles every outcome. */ }
		if (!_stopCts.IsCancellationRequested) { _mailboxProcessor.Post(new FinishedCommand(tracker)); }
	}
	private void StopCore()
	{
		if (_tracker is { } tracker) { tracker.Stop(); }
		else { NotifyWalletStoppedCoinJoin(); }
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

	private CoinSelectionResult SelectCandidateCoins(Wallet wallet)
	{
		var result = GetCoinSelection(wallet);

		if (result.CandidateCoins.Length > 0)
		{
			return result;
		}

		throw new CoinJoinClientException(GetUnavailableNonPrivateCoinsError(result, wallet), "No candidate coins available for coinjoin.");
	}


	private static CoinjoinError GetUnavailableNonPrivateCoinsError(CoinSelectionResult result, Wallet wallet)
	{
		bool AnyNonPrivate(SmartCoin[] coins) => coins.Any(x => !x.IsPrivate(wallet.AnonScoreTarget));

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

	private async ValueTask CancelRestartAsync()
	{
		if (_restart is not { } restart) { return; }
		_restart = null;
		try { await restart.Cancellation.CancelAsync().ConfigureAwait(false); await restart.Task.ConfigureAwait(false); }
		finally { restart.Cancellation.Dispose(); }
	}
	private async Task ScheduleRestartAutomaticallyAsync(bool stopWhenAllMixed, bool overridePlebStop, CancellationToken cancel, TimeSpan? delay = null)
	{
		await CancelRestartAsync().ConfigureAwait(false);
		if (cancel.IsCancellationRequested || _paused || _shutdownHold || _sendHolds > 0) { return; }
		_stopWhenAllMixed = stopWhenAllMixed;
		_overridePlebStop = overridePlebStop;
		var id = Guid.NewGuid();
		// Ownership is transferred to PendingRestart and released by CancelRestart.
#pragma warning disable CA2000
		var cancellation = CancellationTokenSource.CreateLinkedTokenSource(cancel);
#pragma warning restore CA2000
		var task = ScheduleAsync(id, delay ?? TimeSpan.FromSeconds(30), cancellation.Token);
		_restart = new(id, stopWhenAllMixed, overridePlebStop, cancellation, task);
		NotifyWalletStartedCoinJoin();
	}
	private async Task ScheduleAsync(Guid id, TimeSpan delay, CancellationToken cancel)
	{
		try { await Task.Delay(delay, cancel).ConfigureAwait(false); _mailboxProcessor.Post(new RestartCommand(id)); }
		catch (OperationCanceledException) when (cancel.IsCancellationRequested) { }
	}
	private void UpdateSnapshot()
	{
		var state = _tracker is { } tracker ? tracker.InCriticalCoinJoinState ? CoinJoinClientState.InCriticalPhase : CoinJoinClientState.InProgress
			: _restart is not null || _resumeWhenReady && !_paused && !_shutdownHold && _sendHolds == 0 ? CoinJoinClientState.InSchedule : CoinJoinClientState.Idle;
		var snapshot = new CoinJoinSnapshot(state, _tracker?.CoinsInCriticalPhase ?? [], _tracker?.StopWhenAllMixed ?? _restart?.StopWhenAllMixed ?? true, _overridePlebStop, _tracker?.InputRegistrationStarted ?? false, _sendHolds > 0, _shutdownHold);
		lock (_snapshotGate)
		{
			if (_snapshot == snapshot) { return; }
			Volatile.Write(ref _snapshot, snapshot);
			SnapshotChanged.SafeInvoke(this, snapshot);
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
				Logger.LogInfo(FormatLog($"{nameof(CoinJoinClient)} finished. Coinjoin transaction was not broadcast.", wallet));
			}
		}
		catch (UnknownRoundEndingException ex)
		{
			// The round ending is unknown - the transaction might have been broadcast.
			// Payments are already in signed state (moved by TransactionSigned event).
			// The reconciliation process will later check if the transaction was confirmed.
			unknownEnding = true;
			_coinRefrigerator.Freeze(ex.Coins);
			MarkDestinationsUsed(destinationProvider, ex.OutputScripts);
			Logger.LogWarning(FormatLog($"Round ending unknown - payments in signed state awaiting resolution: {ex.Message}", wallet));
		}
		catch (CoinJoinClientException clientException)
		{
			cjClientException = clientException;
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
				Logger.LogInfo($"{nameof(CoinJoinClient)} was cancelled.", wallet);
			}
		}
		catch (UnexpectedRoundPhaseException e)
		{
			// `UnexpectedRoundPhaseException` indicates an error in the protocol however,
			// temporarily we are shortening the circuit by aborting the rounds if
			// there are Alices that didn't confirm.
			// The fix is already done but the clients have to upgrade.
			Logger.LogInfo(FormatLog($"{nameof(CoinJoinClient)} failed with exception: '{e}'", wallet));
		}
		catch (WabiSabiProtocolException wpe) when (wpe.ErrorCode == WabiSabiProtocolErrorCode.WrongPhase)
		{
			// This can happen when the coordinator aborts the round in Signing phase because of detected double spend.
			Logger.LogInfo(FormatLog($"{nameof(CoinJoinClient)} failed with: '{wpe.Message}'", wallet));
		}
		catch (Exception e)
		{
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

		// When to stop mixing:
		// - If stop was requested by user.
		// - If cancellation was requested.
		if (forceStop) { _paused = true; _startRequested = false; }
		if (forceStop
			|| finishedCoinJoin.IsStopped
			|| cancellationToken.IsCancellationRequested)
		{
			NotifyWalletStoppedCoinJoin();
		}
		else if (wallet.IsWalletPrivate() && !wallet.BatchedPayments.AreTherePendingPayments)
		{
			// A fully private wallet is done mixing, unless it is allowed to fund a pending payment with private coins.
			NotifyCoinJoinStartError( CoinjoinError.AllCoinsPrivate);
			if (!finishedCoinJoin.StopWhenAllMixed)
			{
				// In auto CJ mode we never stop trying.
				await ScheduleRestartAutomaticallyAsync(finishedCoinJoin.StopWhenAllMixed, finishedCoinJoin.OverridePlebStop, cancellationToken).ConfigureAwait(false);
			}
			else
			{
				// We finished with CJ permanently.
				NotifyWalletStoppedCoinJoin();
			}
		}
		else if (cjClientException is not null)
		{
			// - If there was a CjClient exception, for example PlebStop or no coins to mix,
			// Keep trying, so CJ starts automatically when the wallet becomes mixable again.
			await ScheduleRestartAutomaticallyAsync(finishedCoinJoin.StopWhenAllMixed, finishedCoinJoin.OverridePlebStop, cancellationToken).ConfigureAwait(false);
			NotifyCoinJoinStartError( cjClientException.CoinjoinError);
		}
		else
		{
			Logger.LogInfo(FormatLog($"{nameof(CoinJoinClient)} restart automatically.", wallet));

			await ScheduleRestartAutomaticallyAsync(finishedCoinJoin.StopWhenAllMixed, finishedCoinJoin.OverridePlebStop, cancellationToken).ConfigureAwait(false);
		}

		finishedCoinJoin.WalletCoinJoinProgressChanged -= CoinJoinTracker_WalletCoinJoinProgressChanged;
		finishedCoinJoin.Dispose();
		_tracker = null;
		if (_startRequested && !_paused && !_shutdownHold && _sendHolds == 0 && !cancellationToken.IsCancellationRequested)
		{
			_startRequested = false;
			await ScheduleRestartAutomaticallyAsync(_stopWhenAllMixed, _overridePlebStop, cancellationToken, TimeSpan.Zero).ConfigureAwait(false);
		}
		UpdateSnapshot();
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
	private record StartCommand(bool StopWhenAllMixed, bool OverridePlebStop) : CoinJoinCommand;
	private record StopCommand : CoinJoinCommand;
	private record ClearPlebOverrideCommand : CoinJoinCommand;
	private record TickCommand : CoinJoinCommand;
	private record RestartCommand(Guid Id) : CoinJoinCommand;
	private record FinishedCommand(CoinJoinTracker Tracker) : CoinJoinCommand;
	private record ProgressCommand(CoinJoinTracker Tracker, CoinJoinProgressEventArgs Args) : CoinJoinCommand;
	private record EnterSendCommand : CoinJoinCommand;
	private record LeaveSendCommand : CoinJoinCommand;
	private abstract record AwaitableCommand(TaskCompletionSource Completion) : CoinJoinCommand;
	private record BeginSendingCommand(TaskCompletionSource Completion) : AwaitableCommand(Completion);
	private record ShutdownCommand(TaskCompletionSource Completion) : AwaitableCommand(Completion);
	private record ResumeCommand(TaskCompletionSource Completion) : AwaitableCommand(Completion);
	private record PendingRestart(Guid Id, bool StopWhenAllMixed, bool OverridePlebStop, CancellationTokenSource Cancellation, Task Task);
}

public record CoinJoinConfiguration(string CoordinatorIdentifier, decimal MaxCoinJoinMiningFeeRate, int AbsoluteMinInputCount, bool AllowSoloCoinjoining);

public record CoinJoinSnapshot(CoinJoinClientState State, ImmutableList<SmartCoin> CriticalCoins, bool StopWhenAllMixed, bool OverridePlebStop, bool IsRunning, bool SendRestricted, bool ShutdownRestricted);
