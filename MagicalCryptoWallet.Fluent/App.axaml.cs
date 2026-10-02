using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Fluent.Providers;
using Avalonia.Threading;
using System.Linq;
using System.Reactive.Concurrency;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using MagicalCryptoWallet.Announcements;
using MagicalCryptoWallet.Fluent.Models.ClientConfig;
using MagicalCryptoWallet.Fluent.Models.FileSystem;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar.Sources;

namespace MagicalCryptoWallet.Fluent;

public class App : Application
{
	private readonly bool _startInBg;
	private readonly DesktopActivation? _activation;
	private readonly Func<Task>? _backendInitializeAsync;
	private ApplicationStateManager? _applicationStateManager;

	public App()
	{
		Name = "Magical Crypto Wallet";
	}

	public App(Func<Task> backendInitializeAsync, bool startInBg, DesktopActivation? activation = null) : this()
	{
		_startInBg = startInBg;
		_activation = activation;
		_backendInitializeAsync = backendInitializeAsync;
	}

	public override void Initialize()
	{
		AvaloniaXamlLoader.Load(this);
	}

	public override void OnFrameworkInitializationCompleted()
	{
		if (!Design.IsDesignMode)
		{
			if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop)
			{
				var uiContext = CreateUiContext();
				var mainViewModel = new MainViewModel(uiContext);
				_applicationStateManager = new ApplicationStateManager(desktop, uiContext, mainViewModel, _startInBg);
				var applicationViewModel = _applicationStateManager.ApplicationViewModel;
				_activation?.Bind(() => Dispatcher.UIThread.Post(() => ((IMainWindowService)_applicationStateManager).Show()));
				DataContext = applicationViewModel;

				desktop.ShutdownMode = ShutdownMode.OnExplicitShutdown;
				desktop.Exit += (sender, args) =>
				{
					mainViewModel.ClearStacks();
					uiContext.HealthMonitor.Dispose();
					mainViewModel.Notifications.Dispose();
					mainViewModel.NavBar.Dispose();
					uiContext.WalletSetupService.Dispose();
				};

				RxApp.MainThreadScheduler.Schedule(
					async () =>
					{
						await Task.Run(_backendInitializeAsync!); // Local disk initialization must not block the first dashboard frame.

						mainViewModel.Initialize();
					});

				InitializeTrayIcons();
			}
		}

		base.OnFrameworkInitializationCompleted();
#if DEBUG
		this.AttachDevTools();
#endif
	}

	private void InitializeTrayIcons()
	{
		// TODO: This is temporary workaround until https://github.com/WalletWasabi/WalletWasabi/issues/8151 is fixed.
		var trayIcons = TrayIcon.GetIcons(this);
		if (trayIcons is not null && trayIcons.FirstOrDefault() is { } trayIcon)
		{
			if (this.TryFindResource("DefaultNativeMenu", out var nativeMenu))
			{
				trayIcon.Menu = nativeMenu as NativeMenu;
			}
		}
	}

	private static WalletSetupService CreateWalletSetupService(IServices services, AmountProvider amountProvider)
	{
		return new WalletSetupService(services, amountProvider);
	}

	private static FileSystemModel CreateFileSystem()
	{
		return new FileSystemModel();
	}

	private static ClientConfigModel CreateConfig(IServices services)
	{
		return new ClientConfigModel(services);
	}

	private static ApplicationSettings CreateApplicationSettings(IServices services)
	{
		return new ApplicationSettings(services, services.PersistentConfig, services.Config, services.UiConfig);
	}

	private static AmountProvider CreateAmountProvider(IServices services)
	{
		return new AmountProvider(services);
	}

	private UiContext CreateUiContext()
	{
		var services = Services.Instance;
		var amountProvider = CreateAmountProvider(services);

		var applicationSettings = CreateApplicationSettings(services);
		var torStatusChecker = new TorStatusCheckerModel(services);

		// This class (App) represents the actual Avalonia Application and it's sole presence means we're in the actual runtime context (as opposed to unit tests)
		// Once all ViewModels have been refactored to receive UiContext as a constructor parameter, this static singleton property can be removed.
		return new UiContext(
			services,
			new QrCodeGenerator(),
			new QrCodeReader(),
			new UiClipboard(),
			CreateWalletSetupService(services, amountProvider),
			new CoinjoinModel(services),
			CreateFileSystem(),
			CreateConfig(services),
			applicationSettings,
			amountProvider,
			new EditableSearchSource(),
			torStatusChecker,
			new HealthMonitor(services, torStatusChecker),
			new ReleaseHighlights(),
			services.Scheme);
	}
}
