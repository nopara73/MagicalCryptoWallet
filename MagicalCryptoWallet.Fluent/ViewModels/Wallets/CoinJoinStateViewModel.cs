using System.Reactive.Disposables.Fluent;
using System.Reactive.Disposables;
using MagicalCryptoWallet.Fluent.Helpers;
using System.Reactive.Linq;
using System.Threading.Tasks;
using System.Windows.Input;
using Avalonia.Threading;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Settings;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoinProgressEvents;
using MagicalCryptoWallet.WabiSabi.Client.StatusChangedEvents;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.Wallets;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets;

[AppLifetime]
public partial class CoinJoinStateViewModel : ViewModelBase, IDisposable
{
	private readonly CompositeDisposable _lifetime = new();
	private const string WaitingMessage = "Awaiting coinjoin";
	private const string CoinjoinMiningFeeRateTooHighMessage = "Mining fee rate was too high";
	private const string MinInputCountTooLowMessage = "Min input count was too low";
	private const string PauseMessage = "Coinjoin is paused";
	private const string PressPlayToStartMessage = "Press Play to start";
	private const string RoundSucceedMessage = "Coinjoin successful! Continuing...";
	private const string RoundFinishedMessage = "Round ended, awaiting next round";
	private const string AbortedNotEnoughAlicesMessage = "Insufficient participants, retrying...";
	private const string CoinJoinInProgress = "Coinjoin in progress";
	private const string InputRegistrationMessage = "Awaiting other participants";
	private const string WaitingForBlameRoundMessage = "Awaiting the blame round";
	private const string WaitingRoundMessage = "Awaiting a round";
	private const string PlebStopMessage = "Coinjoin may be uneconomical";
	private const string PlebStopMessageBelow = "Add more funds or click to continue";
	private const string PlebStopMessageBelowUnconfirmed = "Wait for confirmation or click to continue";
	private const string NoCoinsEligibleToMixMessage = "Insufficient funds eligible for coinjoin";
	private const string UserInSendWorkflowMessage = "Awaiting closure of send dialog";
	private const string AllCoinsArePrivate = "All coins are private";
	private const string GeneralErrorMessage = "Awaiting valid conditions";
	private const string WaitingForConfirmedFunds = "Awaiting confirmed funds";
	private const string CoinsRejectedMessage = "Some funds are rejected from coinjoining";
	private const string OnlyImmatureCoinsAvailableMessage = "Only immature funds are available";
	private const string CoordinatorLiedMessage = "Coordinator lied and might be malicious!";


	private readonly IWalletModel _wallet;
	private readonly Wallet _walletInstance;
	private readonly DispatcherTimer _countdownTimer;
	private CoinJoinSnapshot? _snapshot;
	private DateTimeOffset _countDownStartTime;
	private DateTimeOffset _countDownEndTime;

	[AutoNotify] private bool _playVisible;
	[AutoNotify] private bool _pauseVisible;
	[AutoNotify] private string _currentStatus = "";
	[AutoNotify] private double _progressValue;
	[AutoNotify] private string _leftText = "";
	[AutoNotify] private string _rightText = "";
	[AutoNotify] private bool _isInCriticalPhase;
	[AutoNotify] private bool _isCountDownDelayHappening;
	[AutoNotify] private bool _areAllCoinsPrivate;
	[AutoNotify] private bool _isCoinjoinSupported;

	public CoinJoinStateViewModel(UiContext uiContext, IWalletModel wallet, Wallet walletInstance, WalletCoinjoinModel walletCoinjoinModel, WalletSettingsViewModel settings) : base(uiContext)
	{
		_wallet = wallet;
		_walletInstance = walletInstance;
		IsCoinjoinSupported = wallet.Coinjoin is not null;
		_countdownTimer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
		_countdownTimer.Tick += (_, _) => UpdateCountDown();
		wallet.Privacy.IsWalletPrivate.Subscribe(isPrivate => { AreAllCoinsPrivate = isPrivate; RefreshState(); }).DisposeWith(_lifetime);

		PlayCommand = ReactiveCommand.CreateFromTask(async () =>
		{
			if (UiContext.Services.WalletSession.Snapshot.CoinJoinRequiresAuthorization)
			{
				using var authorization = await AuthorizationHelpers.AuthorizeAsync(UiContext, wallet, "Authorize CoinJoin");
				if (authorization is null) { return; }
				UiContext.Services.WalletSession.AuthorizeCoinJoin(authorization);
			}
			if (!UiContext.Services.WalletSession.Snapshot.IsSynchronized) { return; }
			await walletCoinjoinModel.StartAsync(IsBalanceStop(_snapshot?.WaitingReason));
		}, wallet.Status.Select(status => status.HasCachedData && IsCoinjoinSupported && (status.IsSynchronized || status.CoinJoinRequiresAuthorization)));
		StopPauseCommand = ReactiveCommand.CreateFromTask(walletCoinjoinModel.StopAsync, this.WhenAnyValue(x => x.IsInCriticalPhase).Select(critical => !critical));
		walletCoinjoinModel.Snapshots.Subscribe(snapshot => { _snapshot = snapshot; RefreshState(); }).DisposeWith(_lifetime);
		walletCoinjoinModel.StatusUpdated.Subscribe(status =>
		{
			if (status is CoinJoinStatusEventArgs progress) { OnCoinJoinPhaseChanged(progress.CoinJoinProgressEventArgs); }
		}).DisposeWith(_lifetime);
		wallet.Status.Subscribe(_ => RefreshState()).DisposeWith(_lifetime);
		UiContext.Services.EventBus.AsObservable<PaymentBatchChanged>().ObserveOn(RxApp.MainThreadScheduler).Subscribe(_ => RefreshState()).DisposeWith(_lifetime);
		NavigateToSettingsCommand = ReactiveCommand.Create(() =>
		{
			settings.SelectedTab = 1;
			UiContext.Navigate(NavigationTarget.DialogScreen).To(settings);
		}, Observable.Return(IsCoinjoinSupported));
		CanNavigateToCoinjoinSettings = NavigateToSettingsCommand.CanExecute;
		NavigateToCoordinatorSettingsCommand = ReactiveCommand.CreateFromTask(async () =>
		{
			if (UiContext.MainViewModel is { } mainViewModel) { await mainViewModel.SettingsPage.ActivateCoordinatorTabAsync(); }
		});
		CoinJoinPaymentsCommand = ReactiveCommand.Create(() => UiContext.Navigate(NavigationTarget.DialogScreen).To().CoinJoinPayments(_wallet, _walletInstance));
	}

	public IObservable<bool> CanNavigateToCoinjoinSettings { get; }
	public ReactiveCommand<System.Reactive.Unit, System.Reactive.Unit> NavigateToSettingsCommand { get; }
	public ICommand PlayCommand { get; }
	public ICommand StopPauseCommand { get; }
	public ICommand NavigateToCoordinatorSettingsCommand { get; }
	public ICommand CoinJoinPaymentsCommand { get; }
	private bool IsCounting => _countdownTimer.IsEnabled;
	private bool IsCountDownFinished => GetRemainingTime() <= TimeSpan.Zero;
	private static bool IsBalanceStop(CoinjoinError? error) => error is CoinjoinError.NotEnoughUnprivateBalance or CoinjoinError.NotEnoughConfirmedUnprivateBalance;

	private void RefreshState()
	{
		if (_snapshot is not { } snapshot) { return; }
		IsInCriticalPhase = snapshot.State == CoinJoinClientState.InCriticalPhase;
		var session = UiContext.Services.WalletSession.Snapshot;
		PlayVisible = IsCoinjoinSupported && !IsInCriticalPhase && (snapshot.IsPaused || session.CoinJoinRequiresAuthorization || IsBalanceStop(snapshot.WaitingReason));
		PauseVisible = IsCoinjoinSupported && !snapshot.IsPaused;
		if (snapshot.IsPaused && !IsInCriticalPhase)
		{
			StopCountDown();
			CurrentStatus = snapshot.WaitingReason == CoinjoinError.CoordinatorLiedAboutInputs ? CoordinatorLiedMessage : PauseMessage;
			LeftText = PressPlayToStartMessage;
			return;
		}
		if (session.CoinJoinRequiresAuthorization)
		{
			StopCountDown();
			CurrentStatus = "Awaiting CoinJoin authorization";
			return;
		}
		if (!session.IsSynchronized)
		{
			StopCountDown();
			CurrentStatus = session.State.ToString();
			return;
		}
		if (snapshot.State is CoinJoinClientState.InProgress or CoinJoinClientState.InCriticalPhase) { return; }
		if (!snapshot.SendRestricted && !snapshot.ShutdownRestricted && snapshot.RetryAfter > DateTimeOffset.UtcNow)
		{
			CurrentStatus = "Retrying coinjoin";
			if (_countDownEndTime != snapshot.RetryAfter)
			{
				_countDownStartTime = DateTimeOffset.UtcNow;
				_countDownEndTime = snapshot.RetryAfter;
			}
			_countdownTimer.Start();
			UpdateCountDown();
			return;
		}
		StopCountDown();
		CurrentStatus = snapshot.SendRestricted ? UserInSendWorkflowMessage : snapshot.ShutdownRestricted ? "Coinjoin is on hold" : snapshot.WaitingReason switch
		{
			CoinjoinError.NotEnoughUnprivateBalance or CoinjoinError.NotEnoughConfirmedUnprivateBalance => PlebStopMessage,
			CoinjoinError.NoCoinsEligibleToMix => NoCoinsEligibleToMixMessage,
			CoinjoinError.NoConfirmedCoinsEligibleToMix => WaitingForConfirmedFunds,
			CoinjoinError.AllCoinsPrivate => AllCoinsArePrivate,
			CoinjoinError.CoinsRejected => CoinsRejectedMessage,
			CoinjoinError.OnlyImmatureCoinsAvailable => OnlyImmatureCoinsAvailableMessage,
			CoinjoinError.MiningFeeRateTooHigh => CoinjoinMiningFeeRateTooHighMessage,
			CoinjoinError.MinInputCountTooLow => MinInputCountTooLowMessage,
			_ => WaitingMessage
		};
		if (IsBalanceStop(snapshot.WaitingReason))
		{
			LeftText = snapshot.WaitingReason == CoinjoinError.NotEnoughConfirmedUnprivateBalance ? PlebStopMessageBelowUnconfirmed : PlebStopMessageBelow;
		}
		if (snapshot.WaitingReason == CoinjoinError.AllCoinsPrivate) { ProgressValue = 100; }
	}

	private void UpdateCountDown()
	{
		IsCountDownDelayHappening = IsCounting && IsCountDownFinished;
		if (IsCountDownDelayHappening)
		{
			LeftText = "Waiting for response";
			RightText = "";
			return;
		}
		var format = @"hh\:mm\:ss";
		LeftText = GetElapsedTime().ToString(format);
		RightText = $"-{GetRemainingTime().ToString(format)}";
		ProgressValue = GetPercentage();
	}
	private TimeSpan GetElapsedTime() => DateTimeOffset.UtcNow - _countDownStartTime;
	private TimeSpan GetRemainingTime() => _countDownEndTime - DateTimeOffset.UtcNow;
	private double GetPercentage() => _countDownEndTime <= _countDownStartTime ? 100 : Math.Clamp(GetElapsedTime().TotalSeconds / (_countDownEndTime - _countDownStartTime).TotalSeconds * 100, 0, 100);

	private void OnCoinJoinPhaseChanged(CoinJoinProgressEventArgs coinJoinProgress)
	{
		switch (coinJoinProgress)
		{
			case RoundEnded roundEnded:
				if (roundEnded.IsStopped)
				{
					StopCountDown();
				}
				else
				{
					CurrentStatus = roundEnded.LastRoundState.EndRoundState switch
					{
						EndRoundState.TransactionBroadcasted => RoundSucceedMessage,
						EndRoundState.AbortedNotEnoughAlices => AbortedNotEnoughAlicesMessage,
						_ => RoundFinishedMessage
					};
					StopCountDown();
				}
				break;

			case EnteringOutputRegistrationPhase outputRegPhase:
				ContinueCountDown(outputRegPhase.TimeoutAt - outputRegPhase.RoundState.CoinjoinState.Parameters.OutputRegistrationTimeout,
					outputRegPhase.TimeoutAt + outputRegPhase.RoundState.CoinjoinState.Parameters.TransactionSigningTimeout);
				break;

			case EnteringSigningPhase signingPhase:
				ContinueCountDown(signingPhase.TimeoutAt - signingPhase.RoundState.CoinjoinState.Parameters.TransactionSigningTimeout, signingPhase.TimeoutAt);
				break;

			case EnteringInputRegistrationPhase inputRegPhase:
				StartCountDown(
					message: InputRegistrationMessage,
					start: inputRegPhase.TimeoutAt - inputRegPhase.RoundState.InputRegistrationTimeout,
					end: inputRegPhase.TimeoutAt);
				break;

			case WaitingForBlameRound waitingForBlameRound:
				StartCountDown(message: WaitingForBlameRoundMessage, start: DateTimeOffset.UtcNow, end: waitingForBlameRound.TimeoutAt);
				break;

			case WaitingForRound:
				CurrentStatus = WaitingRoundMessage;
				StopCountDown();
				break;

			case EnteringConnectionConfirmationPhase confirmationPhase:

				var startTime = confirmationPhase.TimeoutAt - confirmationPhase.RoundState.CoinjoinState.Parameters.ConnectionConfirmationTimeout;
				var totalEndTime = confirmationPhase.TimeoutAt +
								   confirmationPhase.RoundState.CoinjoinState.Parameters.OutputRegistrationTimeout +
								   confirmationPhase.RoundState.CoinjoinState.Parameters.TransactionSigningTimeout;

				StartCountDown(
					message: CoinJoinInProgress,
					start: startTime,
					end: totalEndTime);

				break;

			case EnteringCriticalPhase:
				IsInCriticalPhase = true;
				break;

			case LeavingCriticalPhase:
				IsInCriticalPhase = false;
				break;
		}
	}

	private void ContinueCountDown(DateTimeOffset start, DateTimeOffset end)
	{
		// A window opened during a round must initialize its phase countdown too.
		StartCountDown(CoinJoinInProgress, IsCounting ? _countDownStartTime : start, end);
	}

	private void StartCountDown(string message, DateTimeOffset start, DateTimeOffset end)
	{
		CurrentStatus = message;
		_countDownStartTime = start;
		_countDownEndTime = end;
		UpdateCountDown(); // force the UI to apply the changes at the same time.
		_countdownTimer.Start();
	}

	private void StopCountDown()
	{
		_countdownTimer.Stop();
		IsCountDownDelayHappening = false;
		_countDownStartTime = DateTimeOffset.MinValue;
		_countDownEndTime = DateTimeOffset.MinValue;
		LeftText = "";
		RightText = "";
		ProgressValue = 0;
	}
	public void Dispose() { _lifetime.Dispose(); _countdownTimer.Stop(); }

}
