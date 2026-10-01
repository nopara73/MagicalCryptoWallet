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
using MagicalCryptoWallet.Fluent.ViewModels.AddWallet;
using MagicalCryptoWallet.Fluent.ViewModels.HelpAndSupport;
using MagicalCryptoWallet.Fluent.Views.AddWallet;
using MagicalCryptoWallet.Fluent.Views.HelpAndSupport;
using MagicalCryptoWallet.Fluent.Views.Shell;

// Render the actual application views and resources with an inert context.
// No wallet services, networking, navigation, or user data are initialized.
AppBuilder.Configure<App>().WithInterFont().With(new FontManagerOptions { DefaultFamilyName = "fonts:Inter#Inter, $Default" }).UseSkia().UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false }).SetupWithoutStarting();
string destination = args.FirstOrDefault() ?? ".artifacts/rebrand/screenshots";
Directory.CreateDirectory(destination);
var context = (UiContext)System.Runtime.CompilerServices.RuntimeHelpers.GetUninitializedObject(typeof(UiContext));
foreach (var theme in new[] { ThemeVariant.Light, ThemeVariant.Dark })
{
    Application.Current!.RequestedThemeVariant = theme;
    foreach (double scale in new[] { 1.0, 1.25, 1.5, 2.0 })
    {
        Render("welcome", new WelcomePageView { DataContext = new WelcomePageViewModel(context) }, 1024, 680);
        Render("about", new AboutView { DataContext = new AboutViewModel(context) }, 640, 560);
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
Console.WriteLine("Rendered actual Welcome, About, and title bar views in both themes at 100, 125, 150, and 200 percent.");
