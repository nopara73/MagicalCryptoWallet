using System.Reactive.Concurrency;
using System.Reactive.Linq;
using System.Reactive.Subjects;
using System.Threading.Tasks;
using Avalonia.Controls;
using NBitcoin;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.Base;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.ReleaseHighlights;
using MagicalCryptoWallet.Fluent.ViewModels.NavBar;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar.Sources;
using MagicalCryptoWallet.Fluent.ViewModels.Settings;
using MagicalCryptoWallet.Fluent.ViewModels.StatusIcon;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Notifications;
using MagicalCryptoWallet.Helpers;
using System.Diagnostics.CodeAnalysis;

namespace MagicalCryptoWallet.Fluent.ViewModels;

[AppLifetime]
public partial class MainViewModel : ViewModelBase
{
	[AutoNotify] private string _title = "Magical Crypto Wallet";
	[AutoNotify] private WindowState _windowState;
	[AutoNotify] private bool _isOobeBackgroundVisible;
	[AutoNotify] private bool _isCoinJoinActive;

	public MainViewModel(UiContext uiContext) : base(uiContext)
	{
		UiContext.SetMainViewModel(this);

		ApplyUiConfigWindowState();

		DialogScreen = new DialogScreenViewModel(UiContext);
		FullScreen = new DialogScreenViewModel(UiContext, NavigationTarget.FullScreen);
		CompactDialogScreen = new DialogScreenViewModel(UiContext, NavigationTarget.CompactDialogScreen);
		NavBar = new NavBarViewModel(UiContext);
		MainScreen = new TargettedNavigationStack(UiContext, NavigationTarget.HomeScreen);
		UiContext.RegisterNavigation(new NavigationState(UiContext, MainScreen, DialogScreen, FullScreen, CompactDialogScreen, NavBar));

		NavBar.Activate();

		StatusIcon = new StatusIconViewModel(UiContext);

		SettingsPage = new SettingsPageViewModel(UiContext);
		LurkingWifeMode = new LurkingWifeModeViewModel(UiContext, UiContext.ApplicationSettings);
		Notifications = new WalletNotificationsViewModel(UiContext, NavBar);

		NavigationManager.RegisterType(NavBar);

		this.RegisterAllViewModels(UiContext);

		RxApp.MainThreadScheduler.Schedule(async () => await NavBar.InitialiseAsync());

		this.WhenAnyValue(x => x.WindowState)
			.Where(state => state != WindowState.Minimized)
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(state => UiContext.ApplicationSettings.WindowState = state);

		IsMainContentEnabled =
			this.WhenAnyValue(
					x => x.DialogScreen.IsDialogOpen,
					x => x.FullScreen.IsDialogOpen,
					x => x.CompactDialogScreen.IsDialogOpen,
					(dialogIsOpen, fullScreenIsOpen, compactIsOpen) => !(dialogIsOpen || fullScreenIsOpen || compactIsOpen))
				.ObserveOn(RxApp.MainThreadScheduler);

		CurrentWallet =
			this.WhenAnyValue(x => x.MainScreen.CurrentPage)
				.WhereNotNull()
				.OfType<WalletViewModel>();

		IsOobeBackgroundVisible = UiContext.ApplicationSettings.Oobe;
		var isFirstLaunch = !UiContext.WalletRepository.HasWallet || UiContext.ApplicationSettings.Oobe;

		RxApp.MainThreadScheduler.Schedule(async () =>
		{
			if (isFirstLaunch)
			{
				IsOobeBackgroundVisible = true;

				await UiContext.Navigate().To().WelcomePage().GetResultAsync();

				if (UiContext.WalletRepository.HasWallet)
				{
					UiContext.ApplicationSettings.Oobe = false;
					IsOobeBackgroundVisible = false;
				}
			}

			await Task.Delay(1000);

			var lastVersionHighlightsDisplayed = UiContext.ApplicationSettings.LastVersionHighlightsDisplayed;
			UiContext.ApplicationSettings.LastVersionHighlightsDisplayed = Constants.ClientVersion;
			if (!isFirstLaunch && Constants.ClientVersion > lastVersionHighlightsDisplayed)
			{
				await uiContext.Navigate().NavigateDialogAsync(new ReleaseHighlightsDialogViewModel(UiContext),
					navigationMode: NavigationMode.Clear);
			}
		});

		SearchBar = CreateSearchBar();

		NetworkBadgeName =
			UiContext.ApplicationSettings.Network == Network.Main
			? ""
			: UiContext.ApplicationSettings.Network.Name;
	}

	public IObservable<bool> IsMainContentEnabled { get; }

	public string NetworkBadgeName { get; }

	public IObservable<WalletViewModel> CurrentWallet { get; }

	public TargettedNavigationStack MainScreen { get; }

	public SearchBarViewModel SearchBar { get; }

	public DialogScreenViewModel DialogScreen { get; }
	public DialogScreenViewModel FullScreen { get; }
	public DialogScreenViewModel CompactDialogScreen { get; }
	public NavBarViewModel NavBar { get; }
	public StatusIconViewModel StatusIcon { get; }
	public SettingsPageViewModel SettingsPage { get; }
	public LurkingWifeModeViewModel LurkingWifeMode { get; }
	public WalletNotificationsViewModel Notifications { get; }

	public bool IsDialogOpen()
	{
		return DialogScreen.IsDialogOpen
			   || FullScreen.IsDialogOpen
			   || CompactDialogScreen.IsDialogOpen;
	}

	public void ShowDialogAlert()
	{
		if (CompactDialogScreen.IsDialogOpen)
		{
			CompactDialogScreen.ShowAlert = false;
			CompactDialogScreen.ShowAlert = true;
			return;
		}

		if (DialogScreen.IsDialogOpen)
		{
			DialogScreen.ShowAlert = false;
			DialogScreen.ShowAlert = true;
			return;
		}

		if (FullScreen.IsDialogOpen)
		{
			FullScreen.ShowAlert = false;
			FullScreen.ShowAlert = true;
		}
	}

	public void ClearStacks()
	{
		MainScreen.Clear();
		DialogScreen.Clear();
		FullScreen.Clear();
		CompactDialogScreen.Clear();
	}

	public void Initialize()
	{
		UiContext.WalletRepository.WhenAnyValue(x => x.Wallet)
			.Select(wallet => wallet?.IsCoinjoinRunning ?? Observable.Return(false))
			.Switch()
			.BindTo(this, x => x.IsCoinJoinActive);

		UiContext.Services.EventBus.AsObservable<RpcStatusChanged>()
			.Select(x => x.Status)
			.Where(x => !x.IsOk)
			.Take(1)
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(x => NotificationHelpers.ShowError(
				"Could not connect to Bitcoin RPC",
				$"\n>>Click here to verify Bitcoin RPC settings.<<",
				onClick: () =>
				{
					SettingsPage.SelectedTab = 1; // Bitcoin Tab
					_ = SettingsPage.Activate();
				}));

		Notifications.StartListening();

		if (UiContext.ApplicationSettings.Network != Network.Main)
		{
			Title += $" - {UiContext.ApplicationSettings.Network}";
		}
	}

	public void ApplyUiConfigWindowState()
	{
		WindowState = UiContext.ApplicationSettings.WindowState;
	}

	[SuppressMessage("Reliability", "CA2000:Dispose objects before losing scope", Justification = "Same lifecycle as the application. Won't be disposed separately.")]
	private SearchBarViewModel CreateSearchBar()
	{
		// This subject is created to solve the circular dependency between the sources and SearchBarViewModel
		var querySubject = new Subject<string>();

		var source = new CompositeSearchSource(
			new ActionsSearchSource(UiContext, querySubject),
			new SettingsSearchSource(UiContext, querySubject),
			new TransactionsSearchSource(NavBar, querySubject),
			UiContext.EditableSearchSource);

		var searchBar = new SearchBarViewModel(source);

		var queries = searchBar
			.WhenAnyValue(a => a.SearchText)
			.WhereNotNull();

		UiContext.EditableSearchSource.SetQueries(queries);

		queries
			.Subscribe(querySubject);

		return searchBar;
	}
}
