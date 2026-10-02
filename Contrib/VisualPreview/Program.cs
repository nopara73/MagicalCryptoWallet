using System.Reflection;
using System.Reactive.Concurrency;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Styling;
using Avalonia.Threading;
using ReactiveUI;
using ReactiveUI.Avalonia;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.AddWallet;
using MagicalCryptoWallet.Fluent.ViewModels.HelpAndSupport;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.Authorization;
using MagicalCryptoWallet.Fluent.Views.AddWallet;
using MagicalCryptoWallet.Fluent.Views.HelpAndSupport;
using MagicalCryptoWallet.Fluent.Views.Dialogs;
using MagicalCryptoWallet.Fluent.Views.Dialogs.Authorization;
using MagicalCryptoWallet.Fluent.Views.Shell;

// Render the actual application views and resources with an inert context.
// No wallet services, networking, navigation, or user data are initialized.
AppBuilder.Configure<App>().WithInterFont().With(new FontManagerOptions { DefaultFamilyName = "fonts:Inter#Inter, $Default" }).UseSkia().UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false }).UseReactiveUI().SetupWithoutStarting();
// Headless test detection can override RxApp's thread-local scheduler. Commands
// also use RxSchedulers directly, so initialize both before creating any views.
RxApp.MainThreadScheduler = AvaloniaScheduler.Instance;
RxSchedulers.MainThreadScheduler = AvaloniaScheduler.Instance;
bool callbackOnUiThread = false;
using (Task.Run(() => RxSchedulers.MainThreadScheduler.Schedule(() => callbackOnUiThread = Dispatcher.UIThread.CheckAccess())).GetAwaiter().GetResult())
{
    Dispatcher.UIThread.RunJobs();
    if (!callbackOnUiThread) throw new InvalidOperationException("Headless command notifications must return to the Avalonia UI thread.");
}
Console.WriteLine("Headless UI scheduler check passed: background callbacks return to the Avalonia dispatcher.");
string destination = args.FirstOrDefault(x => !x.StartsWith("--", StringComparison.Ordinal)) ?? ".artifacts/rebrand/screenshots";
Directory.CreateDirectory(destination);
if (args.Contains("--bitcoin-only"))
{
    BitcoinP2pChecks.Render(destination);
    return;
}
if (args.Contains("--recovery-words-only"))
{
    try { RecoveryWordsChecks.Run(destination); }
    catch (Exception error) { Console.Error.WriteLine(error); Environment.ExitCode = 1; }
    return;
}
var context = (UiContext)System.Runtime.CompilerServices.RuntimeHelpers.GetUninitializedObject(typeof(UiContext));
if (args.Contains("--history-dates-only"))
{
    HistoryDateChecks.Run(context, destination);
    return;
}
if (args.Contains("--fees-only"))
{
    var feeServices = (Services)System.Runtime.CompilerServices.RuntimeHelpers.GetUninitializedObject(typeof(Services));
    AutomaticCoinSelectionChecks.SetBackingField(feeServices, nameof(Services.UiConfig), new UiConfig(Path.Combine(Path.GetFullPath(destination), "synthetic-fee-ui-config.json")));
    typeof(Services).GetProperty(nameof(Services.Instance))!.SetValue(null, feeServices);
    AutomaticCoinSelectionChecks.Run(context);
    FeeDisplayChecks.Run(context, destination);
    return;
}
PasswordBoxChecks.Run();
LurkingWifeModeChecks.Initialize(context, destination);
SingleWalletChecks.Run(context);
AutomaticCoinSelectionChecks.Run(context);
using var bitcoinP2p = new BitcoinP2pChecks(destination);
SoftwareWalletChecks.Run(context, destination);
int capturedFrames = 0;
foreach (var theme in new[] { ThemeVariant.Light, ThemeVariant.Dark })
{
    Application.Current!.RequestedThemeVariant = theme;
    foreach (double scale in new[] { 1.0, 1.25, 1.5, 2.0 })
    {
        Render("welcome", new WelcomePageView { DataContext = new WelcomePageViewModel(context) }, 1024, 680);
        Render("about", new AboutView { DataContext = new AboutViewModel(context) }, 640, 560);
        Render("password-create", new CreatePasswordDialogView
        {
            DataContext = new CreatePasswordDialogViewModel(context, "Add Password")
            {
                Password = "synthetic-password", ConfirmPassword = "synthetic-password"
            }
        }, 640, 440);
		Render("password-create-empty", new CreatePasswordDialogView
		{
			DataContext = new CreatePasswordDialogViewModel(context, "Add Password",
				"This password is needed to send bitcoin and recover your wallet.\nStore it safely; it cannot be reset if lost.")
		}, 640, 440);
		var authorizationWallet = DispatchProxy.Create<IWalletModel, InertPreviewWallet>();
        Render("password-auth", new PasswordAuthDialogView
        {
            DataContext = new PasswordAuthDialogViewModel(context, authorizationWallet)
            {
                Password = "synthetic-password"
            }
        }, 640, 440);
		Render("password-auth-empty", new PasswordAuthDialogView
		{
			DataContext = new PasswordAuthDialogViewModel(context, authorizationWallet)
		}, 640, 440);
		Render("password-auth-error", new PasswordAuthDialogView
		{
			DataContext = new PasswordAuthDialogViewModel(context, authorizationWallet)
			{
				HasAuthorizationFailed = true
			}
		}, 640, 440);
        Render("lurking-wife-mode-off", LurkingWifeModeChecks.CreatePreview(context, false), 640, 300);
        Render("lurking-wife-mode-on", LurkingWifeModeChecks.CreatePreview(context, true), 640, 300);
        Services.Instance.UiConfig.PrivacyMode = false;
        Render("single-wallet", SingleWalletChecks.CreatePreview(context), 800, 600);
        Render("wallet-setup", new AddWalletPageView { DataContext = new AddWalletPageViewModel(context) }, 800, 600);
        Render("wallet-actions", AutomaticCoinSelectionChecks.CreateWalletActions(context), 900, 650);
        Render("dashboard-unknown", AutomaticCoinSelectionChecks.CreateWalletActions(context, status: "Loading", hasCachedData: false), 900, 650);
        Render("dashboard-syncing", AutomaticCoinSelectionChecks.CreateWalletActions(context, status: "Syncing"), 900, 650);
        Render("dashboard-offline", AutomaticCoinSelectionChecks.CreateWalletActions(context, status: "Offline"), 900, 650);
        Render("dashboard-faulted", AutomaticCoinSelectionChecks.CreateWalletActions(context, status: "Faulted"), 900, 650);
        Render("transaction-preview", AutomaticCoinSelectionChecks.CreateTransactionPreview(context), 900, 650);
        Render("wallet-coins", AutomaticCoinSelectionChecks.CreateWalletCoins(context), 900, 650);
        Render("wallet-general-settings", AutomaticCoinSelectionChecks.CreateWalletSettings(context), 900, 650);
        Render("bitcoin-settings", bitcoinP2p.CreateSettings(), 650, 340);
        Render("bitcoin-status", bitcoinP2p.CreateStatus(), 360, 560);
        Render("send", SoftwareWalletChecks.CreateSend(context), 900, 650);
        Render("receive", SoftwareWalletChecks.CreateReceive(context), 900, 650);
        Render("recovery", SoftwareWalletChecks.CreateRecovery(context), 900, 650);
        Render("recover-words", SoftwareWalletChecks.CreateRecoverWords(context), 900, 650);
        void Render(string name, Control content, int width, int height)
        {
            var panel = new DockPanel();
            var title = new TitleBar { Height = 46 };
            DockPanel.SetDock(title, Dock.Top);
            panel.Children.Add(title);
            panel.Children.Add(content);
            var window = new Window { Title = "Magical Crypto Wallet", Width = width, Height = height, Content = panel,
                Background = theme == ThemeVariant.Dark ? new SolidColorBrush(Color.Parse("#151515")) : Brushes.White };
            // Avalonia 11's headless backend fixes this property at 1. Simulate the
            // platform's DPI notification so layout and the compositor use the same scale.
            var platform = window.PlatformImpl ?? throw new InvalidOperationException("Missing headless window.");
            var scalingField = platform.GetType().GetField("<RenderScaling>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)
                ?? throw new InvalidOperationException("The pinned headless backend no longer exposes its scaling field.");
            scalingField.SetValue(platform, scale);
            var scalingChanged = platform.GetType().GetProperty("ScalingChanged")?.GetValue(platform) as Action<double>;
            (scalingChanged ?? throw new InvalidOperationException("Missing platform DPI notification."))(scale);
            window.Show();
            Dispatcher.UIThread.RunJobs();
            window.Measure(new Size(width, height));
            window.Arrange(new Rect(0, 0, width, height));
            using var bitmap = window.CaptureRenderedFrame() ?? throw new InvalidOperationException("The compositor did not render a frame.");
            if (window.RenderScaling != scale || bitmap.PixelSize != PixelSize.FromSize(new Size(width, height), scale))
                throw new InvalidOperationException("The captured frame does not match the requested display scale.");
            bitmap.Save(Path.Combine(destination, $"{name}-{theme.Key!.ToString()!.ToLowerInvariant()}-{(int)(scale * 100)}.png"));
            capturedFrames++;
            window.Close();
        }
    }
}
Console.WriteLine($"Rendered {capturedFrames} actual application captures: Welcome, About, password creation/authorization, Lurking Wife Mode, single-wallet sidebar/dashboard, setup, wallet actions, transaction preview, coins, settings, send, receive, recovery and recovery words in both themes at 100, 125, 150 and 200 percent.");
using var syntheticWallets = LurkingWifeModeChecks.Run(context, destination);

// Authorize is never invoked. Any attempt to use a wallet service fails immediately.
public class InertPreviewWallet : DispatchProxy
{
    protected override object? Invoke(MethodInfo? targetMethod, object?[]? args) => targetMethod?.Name switch
	{
		"Dispose" => null,
		_ => throw new InvalidOperationException("Wallet services are unavailable in the visual preview.")
	};
}
