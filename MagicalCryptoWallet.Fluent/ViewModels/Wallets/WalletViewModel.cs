using System.Collections.Generic;
using System.Linq;
using System.Reactive.Concurrency;
using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Input;
using NBitcoin;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.Models.Transactions;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar.SearchItems;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar.Sources;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.History;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.Tiles;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Settings;
using MagicalCryptoWallet.Wallets;
using ScriptType = MagicalCryptoWallet.Fluent.Models.Wallets.ScriptType;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets;

[AppLifetime]
public partial class WalletViewModel : RoutableViewModel, IWalletViewModel, IDisposable
{
	private readonly CompositeDisposable _lifetime = new();
	public static string FindCoordinatorLink { get; } = "https://github.com/nopara73/MagicalCryptoWallet/blob/master/MagicalCryptoWallet.Documentation/README.md";

	[AutoNotify(SetterModifier = AccessModifier.Protected)] private bool _isCoinJoining;

	[AutoNotify(SetterModifier = AccessModifier.Protected)] private bool _isLoading;
	[AutoNotify] private bool _isPointerOver;
	[AutoNotify(SetterModifier = AccessModifier.Private)] private bool _isSendButtonVisible;

	[AutoNotify(SetterModifier = AccessModifier.Private)] private bool _isWalletBalanceZero;
	[AutoNotify(SetterModifier = AccessModifier.Private)] private bool _areAllCoinsPrivate;
	[AutoNotify(SetterModifier = AccessModifier.Private)] private bool _hasMusicBoxBeenDisplayed;
	[AutoNotify] private bool _isMusicBoxFlyoutDisplayed;

	[AutoNotify] private ICommand _defaultReceiveCommand;

	// This proxy fixes a stack overflow bug in Avalonia
	public bool IsMusicBoxFlyoutOpenedProxy
	{
		get => IsMusicBoxFlyoutDisplayed;
		set => IsMusicBoxFlyoutDisplayed = value;
	}

	private string _title = "";
	[AutoNotify(SetterModifier = AccessModifier.Protected)] private bool _hasCachedData;
	[AutoNotify] private bool _canSpend;

	public WalletViewModel(UiContext uiContext, IWalletModel walletModel) : base(uiContext)
	{
		WalletModel = walletModel;
		var wallet = uiContext.Services.WalletSession.GetWallet() ?? throw new InvalidOperationException("No wallet is configured.");

		Settings = new WalletSettingsViewModel(UiContext, WalletModel);
		History = new HistoryViewModel(UiContext, WalletModel);

		var searchItems = CreateSearchItems();
		this.WhenAnyValue(x => x.IsActive)
			.Do(shouldDisplay => UiContext.EditableSearchSource.Toggle(searchItems, shouldDisplay))
			.Subscribe().DisposeWith(_lifetime);

		var sendSearchItem = CreateSendItem();
		this.WhenAnyValue(x => x.IsSendButtonVisible, x => x.IsActive, (x, y) => x && y)
			.Do(shouldAdd => UiContext.EditableSearchSource.Toggle(sendSearchItem, shouldAdd))
			.Subscribe().DisposeWith(_lifetime);


		walletModel.HasBalance
			.Select(x => !x)
			.BindTo(this, x => x.IsWalletBalanceZero).DisposeWith(_lifetime);

		walletModel.IsCoinjoinRunning
			.BindTo(this, x => x.IsCoinJoining).DisposeWith(_lifetime);

		 // Keep the send button visible while Lurking Wife Mode is on. Otherwise its absence reveals an empty wallet.
		 this.WhenAnyValue(x => x.IsWalletBalanceZero, x => x.UiContext.ApplicationSettings.PrivacyMode)
			.Subscribe(_ => IsSendButtonVisible = (!IsWalletBalanceZero || UiContext.ApplicationSettings.PrivacyMode) && (!WalletModel.IsWatchOnlyWallet || WalletModel.IsHardwareWallet)).DisposeWith(_lifetime);


		 WalletModel.Privacy.IsWalletPrivate
			 .BindTo(this, x => x.AreAllCoinsPrivate).DisposeWith(_lifetime);

		 IsMusicBoxVisible = this.WhenAnyValue(
			 x => x.HasMusicBoxBeenDisplayed,
			 x => x.IsActive,
			 x => x.IsWalletBalanceZero,
			 x => x.AreAllCoinsPrivate,
			 x => x.IsPointerOver,
			 x => x.IsMusicBoxFlyoutDisplayed,
			 (hasBeenDisplayed, isActive, hasNoBalance, areAllCoinsPrivate, isPointerOver, isMusicBoxFlyoutDisplayed) =>
			 {
				 if (!hasBeenDisplayed)
				 {
					 if (!WalletModel.IsCoinJoinEnabled)
					 {
						 // If there is no coordinator configured and it's the first time, display MusicBox even without pointer over
						 Task.Run(() => DelaySwitchHasMusicBoxBeenDisplayedAsync(CancellationToken.None));
						 return isActive && !WalletModel.IsCoinJoinEnabled;
					 }

					 HasMusicBoxBeenDisplayed = true;
				 }

				 if (!WalletModel.IsCoinJoinEnabled)
				 {
					 return isActive && !WalletModel.IsCoinJoinEnabled && (isPointerOver || isMusicBoxFlyoutDisplayed);
				 }

				 return isActive && !hasNoBalance && !WalletModel.IsWatchOnlyWallet;
			 });


		SendCommand = ReactiveCommand.Create(() => Navigate().To().Send(walletModel, new SendFlowModel(wallet)), walletModel.Status.Select(x => x.IsSynchronized));

		SegwitReceiveCommand = ReactiveCommand.Create(() => Navigate().To().Receive(WalletModel, ScriptType.SegWit), walletModel.Status.Select(x => x.HasCachedData));
		TaprootReceiveCommand = ReactiveCommand.Create(() => Navigate().To().Receive(WalletModel, ScriptType.Taproot),
			walletModel.Status.Select(x => x.HasCachedData && WalletModel.SeveralReceivingScriptTypes));
		_lifetime.Add((IDisposable)TaprootReceiveCommand);
		_defaultReceiveCommand = SegwitReceiveCommand;

		this.WhenAnyValue(x => x.Settings.DefaultReceiveScriptType)
			.Merge(walletModel.Status.Select(_ => Settings.DefaultReceiveScriptType))
			.Subscribe(value =>
				DefaultReceiveCommand = value == ScriptType.SegWit || !SeveralReceivingScriptTypes
					? SegwitReceiveCommand
					: TaprootReceiveCommand).DisposeWith(_lifetime);

		WalletInfoCommand = ReactiveCommand.Create(() => Navigate().To().WalletInfo(WalletModel));

		WalletStatsCommand = ReactiveCommand.Create(() => Navigate().To().WalletStats(WalletModel));

		WalletSettingsCommand = ReactiveCommand.Create(
			() =>
			{
				Settings.SelectedTab = 0;
				Navigate(NavigationTarget.DialogScreen).To(Settings);
			});

		CoinJoinSettingsCommand = ReactiveCommand.Create(
			() =>
			{
				Settings.SelectedTab = 1;
				Navigate(NavigationTarget.DialogScreen).To(Settings);
			});

		WalletCoinsCommand = ReactiveCommand.Create(() => Navigate(NavigationTarget.DialogScreen).To().WalletCoins(WalletModel));

		CoinJoinStateViewModel = WalletModel.IsCoinJoinEnabled
			? new CoinJoinStateViewModel(uiContext, WalletModel, wallet, WalletModel.Coinjoin!, Settings)
			: null;

		CoinJoinPaymentsCommand = ReactiveCommand.Create(() => Navigate(NavigationTarget.DialogScreen).To().CoinJoinPayments(WalletModel, wallet));

		if (WalletModel.IsCoinJoinEnabled)
		{
			var coinjoinPaymentsSearchItem = CreateCoinJoinPaymentsItem();
			this.WhenAnyValue(x => x.IsActive)
				.Do(shouldDisplay => UiContext.EditableSearchSource.Toggle(coinjoinPaymentsSearchItem, shouldDisplay))
				.Subscribe().DisposeWith(_lifetime);
		}

		NavigateToCoordinatorSettingsCommand = ReactiveCommand.CreateFromTask(async () =>
		{
			if (UiContext.MainViewModel is { } mainViewModel)
			{
				await mainViewModel.SettingsPage.ActivateCoordinatorTabAsync();
			}
		});

		CoordinatorHelpCommand = ReactiveCommand.CreateFromTask(() => UiContext.OpenBrowserAsync("https://github.com/nopara73/MagicalCryptoWallet/blob/master/MagicalCryptoWallet.Documentation/README.md"));

		Tiles = GetTiles().ToList();

		this.WhenAnyValue(x => x.Settings.PreferPsbtWorkflow)
			.Do(x => this.RaisePropertyChanged(nameof(PreferPsbtWorkflow)))
			.Subscribe().DisposeWith(_lifetime);

		Title = "Magical Crypto Wallet";
		SyncStatus = new WalletSyncStatusViewModel(uiContext, walletModel);
		walletModel.Status.Subscribe(status =>
		{
			HasCachedData = status.HasCachedData;
			CanSpend = status.IsSynchronized;
			IsLoading = status.State is WalletSessionState.Loading or WalletSessionState.Syncing;
			this.RaisePropertyChanged(nameof(SeveralReceivingScriptTypes));
		}).DisposeWith(_lifetime);
		OpenCommand = ReactiveCommand.Create(() => UiContext.Navigate().OpenWalletHome());
	}


	public IWalletModel WalletModel { get; }

	public WalletSyncStatusViewModel SyncStatus { get; }
	public ICommand OpenCommand { get; }
	public string IconName => "nav_wallet_24_regular";
	public string IconNameFocused => "nav_wallet_24_filled";

	public bool PreferPsbtWorkflow => WalletModel.Settings.PreferPsbtWorkflow;

	public bool SeveralReceivingScriptTypes => WalletModel.SeveralReceivingScriptTypes;

	public bool IsWatchOnly => WalletModel.IsWatchOnlyWallet;

	public IObservable<bool> IsMusicBoxVisible { get; }

	public CoinJoinStateViewModel? CoinJoinStateViewModel { get; private set; }

	public WalletSettingsViewModel Settings { get; private set; }

	public HistoryViewModel History { get; }

	public IEnumerable<ActivatableViewModel> Tiles { get; }

	public ICommand SendCommand { get; private set; }

	public ICommand? BroadcastPsbtCommand { get; set; }
	public ICommand SegwitReceiveCommand { get; private set; }
	public ICommand? TaprootReceiveCommand { get; private set; }

	public ICommand WalletInfoCommand { get; private set; }

	public ICommand WalletSettingsCommand { get; private set; }

	public ICommand WalletStatsCommand { get; private set; }

	public ICommand WalletCoinsCommand { get; private set; }

	public ICommand CoinJoinSettingsCommand { get; private set; }

	public ICommand CoinJoinPaymentsCommand { get; private set; }

	public ICommand NavigateToCoordinatorSettingsCommand { get; }

	public ICommand CoordinatorHelpCommand { get; }

	public override string Title
	{
		get => _title;
		protected set => this.RaiseAndSetIfChanged(ref _title, value);
	}

	public void SelectTransaction(uint256 txid)
	{
		RxApp.MainThreadScheduler.Schedule(async () =>
		{
			await Task.Delay(500);
			History.SelectTransaction(txid);
		});
	}

	public void NavigateAndHighlight(uint256 txid)
	{
		Navigate().To(this, NavigationMode.Clear);

		SelectTransaction(txid);
	}

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		History.Activate(disposables);

		foreach (var tile in Tiles)
		{
			tile.Activate(disposables);
		}

		WalletModel.Status.Select(x => x.HasCachedData)
			.BindTo(this, x => x.HasCachedData)
			.DisposeWith(disposables);
	}

	private ISearchItem[] CreateSearchItems()
	{
		return new ISearchItem[]
		{
			new ActionableItem("Receive", "Display wallet receive dialog", () => { DefaultReceiveCommand.ExecuteIfCan(); return Task.CompletedTask; }, "Wallet", new[] { "Wallet", "Receive", "Action", }) { Icon = "wallet_action_receive", IsDefault = true, Priority = 2 },
			new ActionableItem("Coinjoin Settings", "Display wallet coinjoin settings", () => { CoinJoinSettingsCommand.ExecuteIfCan(); return Task.CompletedTask; }, "Wallet", new[] { "Wallet", "Settings", }) { Icon = "wallet_action_coinjoin", IsDefault = true, Priority = 3 },
			new ActionableItem("Wallet Settings", "Display wallet settings", () => { WalletSettingsCommand.ExecuteIfCan(); return Task.CompletedTask; }, "Wallet", new[] { "Wallet", "Settings", }) { Icon = "settings_wallet_regular", IsDefault = true, Priority = 4 },
			new ActionableItem("Wallet Coins", "Display wallet coins", () => { WalletCoinsCommand.ExecuteIfCan(); return Task.CompletedTask; }, "Wallet", new[] { "Wallet", "Coins", "UTXO", }) { Icon = "wallet_coins", IsDefault = true, Priority = 6 },
			new ActionableItem("Wallet Stats", "Display wallet stats", () => { WalletStatsCommand.ExecuteIfCan(); return Task.CompletedTask; }, "Wallet", new[] { "Wallet", "Stats", }) { Icon = "stats_wallet_regular", IsDefault = true, Priority = 7 },
			new ActionableItem("Wallet Info", "Display wallet info", () => { WalletInfoCommand.ExecuteIfCan(); return Task.CompletedTask; }, "Wallet", new[] { "Wallet", "Info", }) { Icon = "info_regular", IsDefault = true, Priority = 8 },
		};
	}

	private ISearchItem CreateSendItem()
	{
		return new ActionableItem("Send", "Display wallet send dialog", () => { SendCommand.ExecuteIfCan(); return Task.CompletedTask; }, "Wallet", new[] { "Wallet", "Send", "Action", }) { Icon = "wallet_action_send", IsDefault = true, Priority = 1 };
	}


	private ISearchItem CreateCoinJoinPaymentsItem()
	{
		return new ActionableItem("Coinjoin Payments", "Manage queued coinjoin payments", () => { CoinJoinPaymentsCommand.ExecuteIfCan(); return Task.CompletedTask; }, "Wallet", new[] { "Wallet", "Coinjoin", "Payments", "Send", "Batch" }) { Icon = "embedded_payment", IsDefault = true, Priority = 3 };
	}

	private IEnumerable<ActivatableViewModel> GetTiles()
	{
		yield return new WalletBalanceTileViewModel(UiContext, WalletModel.Balances);

		if (!IsWatchOnly)
		{
			yield return new PrivacyControlTileViewModel(UiContext, WalletModel);
		}

		yield return new BtcPriceTileViewModel(UiContext, UiContext.AmountProvider);
	}

	private async Task DelaySwitchHasMusicBoxBeenDisplayedAsync(CancellationToken cancellationToken)
	{
		await Task.Delay(10000, cancellationToken);
		HasMusicBoxBeenDisplayed = true;
	}
	public void Dispose() { _lifetime.Dispose(); IsActive = false; SyncStatus.Dispose(); CoinJoinStateViewModel?.Dispose(); History.Dispose(); Settings.Dispose(); }

}
