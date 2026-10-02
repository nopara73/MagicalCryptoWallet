using System.Net;
using System.Reflection;
using System.Runtime.CompilerServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Styling;
using Avalonia.Threading;
using NBitcoin;
using NBitcoin.Protocol;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.FeeRateEstimation;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Models;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Settings;
using MagicalCryptoWallet.Fluent.ViewModels.StatusIcon;
using MagicalCryptoWallet.Fluent.Views.Settings;
using MagicalCryptoWallet.Fluent.Views.StatusIcon;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Services;

// Only synthetic events and settings are used. This never starts wallet or network services.
internal sealed class BitcoinP2pChecks : IDisposable
{
    private readonly UiContext _context;
    private readonly EventBus _events = new();
    private readonly HealthMonitor _health;

    public static void Render(string destination)
    {
        using var checks = new BitcoinP2pChecks(destination);
        foreach (var theme in new[] { ThemeVariant.Light, ThemeVariant.Dark })
        {
            Application.Current!.RequestedThemeVariant = theme;
            foreach (double scale in new[] { 1.0, 1.25, 1.5, 2.0 })
            {
                RenderView("bitcoin-settings", checks.CreateSettings(), 650, 340);
                RenderView("bitcoin-status", checks.CreateStatus(), 360, 560);
                void RenderView(string name, Control content, int width, int height)
                {
                    var window = new Window
                    {
                        Title = "Magical Crypto Wallet", Width = width, Height = height, Content = content,
                        Background = theme == ThemeVariant.Dark ? new SolidColorBrush(Color.Parse("#151515")) : Brushes.White
                    };
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
        Console.WriteLine("Rendered actual Bitcoin settings and P2P status in both themes at 100, 125, 150 and 200 percent.");
    }

    public BitcoinP2pChecks(string destination)
    {
        var services = DispatchProxy.Create<IServices, BitcoinPreviewServices>();
        var proxy = (BitcoinPreviewServices)services;
        proxy.Events = _events;
        proxy.ConfigPath = Path.Combine(destination, "synthetic-bitcoin-settings.json");
        var persistent = PersistentConfigManager.DefaultMainNetConfig with { UseTor = "Disabled", DownloadNewVersion = false };
        var settings = new ApplicationSettings(services, persistent, new Config(persistent, []),
            new UiConfig(Path.Combine(destination, "synthetic-bitcoin-ui.json")));
        var tor = new TorStatusCheckerModel(services);
        _health = new HealthMonitor(services, tor);
        _context = (UiContext)RuntimeHelpers.GetUninitializedObject(typeof(UiContext));
        SetContext(nameof(UiContext.Services), services);
        SetContext(nameof(UiContext.ApplicationSettings), settings);
        SetContext(nameof(UiContext.HealthMonitor), _health);
        SetContext(nameof(UiContext.TorStatusChecker), tor);

        _events.Publish(new TorNetworkStatusChanged([]));
        var node = (Node)RuntimeHelpers.GetUninitializedObject(typeof(Node));
        var endpoint = new IPEndPoint(IPAddress.Loopback, 18444);
        AssertState(HealthMonitorState.Loading);
        _events.Publish(new P2pNodeAdded(endpoint, node));
        AssertState(HealthMonitorState.Loading); // A handshake alone is not synchronized.
        _events.Publish(new NetworkTipHeightChanged(100));
        _events.Publish(new ClientTipHeightChanged(99));
        AssertState(HealthMonitorState.Loading);
        _events.Publish(new ClientTipHeightChanged(100));
        AssertState(HealthMonitorState.Ready);
        _events.Publish(new P2pNodeRemoved(endpoint, node));
        AssertState(HealthMonitorState.Loading); // Cached equal tips still require a peer.
        _events.Publish(new P2pNodeAdded(endpoint, node));
        AssertState(HealthMonitorState.Ready);
        _events.Publish(new MiningFeeRatesChanged(new FeeRateEstimations(new Dictionary<int, FeeRate> { [2] = new(2m) })));
        Dispatcher.UIThread.RunJobs();
        Console.WriteLine("Bitcoin P2P health checks passed: initial sync, behind tip, ready, disconnect and reconnect.");
    }

    public Control CreateSettings() => new BitcoinTabSettingsView
    {
        DataContext = new BitcoinTabSettingsViewModel(_context, _context.ApplicationSettings),
        Margin = new Thickness(24)
    };

    public Control CreateStatus()
    {
        var model = new StatusIconViewModel(_context);
        var view = new StatusIcon { DataContext = model };
        var icon = (Control)view.Content!;
        var flyout = (Flyout)FlyoutBase.GetAttachedFlyout(icon)!;
        var details = (Control)flyout.Content!;
        flyout.Content = null;
        details.DataContext = model;
        view.Content = null;
        var panel = new DockPanel { Margin = new Thickness(24) };
        DockPanel.SetDock(icon, Dock.Bottom);
        icon.Height = 40;
        panel.Children.Add(icon);
        panel.Children.Add(new Border { Padding = new Thickness(16), Child = details, HorizontalAlignment = HorizontalAlignment.Center });
        view.Content = panel;
        return view;
    }

    private void AssertState(HealthMonitorState expected)
    {
        // The production health monitor throttles state updates for 100 milliseconds.
        Dispatcher.UIThread.RunJobs();
        var deadline = DateTime.UtcNow + TimeSpan.FromSeconds(3);
        do
        {
            Thread.Sleep(20);
            Dispatcher.UIThread.RunJobs();
        } while (_health.State != expected && DateTime.UtcNow < deadline);
        if (_health.State != expected)
            throw new InvalidOperationException($"Expected P2P health {expected}, got {_health.State}.");
    }

    private void SetContext(string name, object value) => typeof(UiContext)
        .GetField($"<{name}>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(_context, value);

    public void Dispose() => _health.Dispose();
}

public class BitcoinPreviewServices : DispatchProxy
{
    public EventBus Events { get; set; } = null!;
    public string ConfigPath { get; set; } = null!;

    protected override object? Invoke(MethodInfo? targetMethod, object?[]? args) => targetMethod?.Name switch
    {
        "get_EventBus" => Events,
        "get_PersistentConfigFilePath" => ConfigPath,
        "GetUseTor" => TorMode.Disabled,
        _ => throw new InvalidOperationException($"Wallet services are unavailable in the Bitcoin preview: {targetMethod?.Name}.")
    };
}
