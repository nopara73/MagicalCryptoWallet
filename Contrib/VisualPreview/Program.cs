using System.Reflection;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Styling;
using Avalonia.Threading;
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
AppBuilder.Configure<App>().WithInterFont().With(new FontManagerOptions { DefaultFamilyName = "fonts:Inter#Inter, $Default" }).UseSkia().UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false }).SetupWithoutStarting();
string destination = args.FirstOrDefault() ?? ".artifacts/rebrand/screenshots";
Directory.CreateDirectory(destination);
var context = (UiContext)System.Runtime.CompilerServices.RuntimeHelpers.GetUninitializedObject(typeof(UiContext));
PasswordBoxChecks.Run();
using var syntheticWallets = LurkingWifeModeChecks.Run(context, destination);
foreach (var theme in new[] { ThemeVariant.Light, ThemeVariant.Dark })
{
    Application.Current!.RequestedThemeVariant = theme;
    foreach (double scale in new[] { 1.0, 1.25, 1.5, 2.0 })
    {
        Render("welcome", new WelcomePageView { DataContext = new WelcomePageViewModel(context) }, 1024, 680);
        Render("about", new AboutView { DataContext = new AboutViewModel(context) }, 640, 560);
        Render("password-create", new CreatePasswordDialogView
        {
            DataContext = new CreatePasswordDialogViewModel(context, "Add Passphrase")
            {
                Password = "synthetic-passphrase", ConfirmPassword = "synthetic-passphrase"
            }
        }, 640, 440);
        Render("password-auth", new PasswordAuthDialogView
        {
            DataContext = new PasswordAuthDialogViewModel(context, DispatchProxy.Create<IWalletModel, InertPreviewWallet>())
            {
                Password = "synthetic-passphrase"
            }
        }, 640, 440);
        Render("lurking-wife-mode-off", LurkingWifeModeChecks.CreatePreview(context, false), 640, 300);
        Render("lurking-wife-mode-on", LurkingWifeModeChecks.CreatePreview(context, true), 640, 300);
        void Render(string name, Control content, int width, int height)
        {
            var panel = new DockPanel();
            var title = new TitleBar { Height = 46 };
            DockPanel.SetDock(title, Dock.Top);
            panel.Children.Add(title);
            panel.Children.Add(content);
            var window = new Window { Title = "Magical Crypto Wallet", Width = width, Height = height, Content = panel,
                Background = theme == ThemeVariant.Dark ? new SolidColorBrush(Color.Parse("#151515")) : Brushes.White };
            window.Show();
            Dispatcher.UIThread.RunJobs();
            window.Measure(new Size(width, height));
            window.Arrange(new Rect(0, 0, width, height));
            using var bitmap = new RenderTargetBitmap(new PixelSize((int)(width * scale), (int)(height * scale)), new Vector(96 * scale, 96 * scale));
            bitmap.Render(window);
            bitmap.Save(Path.Combine(destination, $"{name}-{theme.Key!.ToString()!.ToLowerInvariant()}-{(int)(scale * 100)}.png"));
            window.Close();
        }
    }
}
Console.WriteLine("Rendered actual Welcome, About, passphrase creation/authorization, Lurking Wife Mode sidebar/masking controls, and title bar views in both themes at 100, 125, 150, and 200 percent.");

// Authorize is never invoked. Any attempt to use a wallet service fails immediately.
public class InertPreviewWallet : DispatchProxy
{
    protected override object? Invoke(MethodInfo? targetMethod, object?[]? args) => targetMethod?.Name == "get_IsHardwareWallet"
        ? false : throw new InvalidOperationException("Wallet services are unavailable in the visual preview.");
}
