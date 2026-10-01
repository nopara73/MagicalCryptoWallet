using System.Reflection;
using System.Diagnostics;
using System.Reactive.Linq;
using System.Reactive.Disposables;
using System.Runtime.CompilerServices;
using Avalonia;
using Avalonia.Automation.Peers;
using Avalonia.Controls;
using Avalonia.Controls.Presenters;
using Avalonia.Headless;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Threading;
using Avalonia.VisualTree;
using ReactiveUI;
using MagicalCryptoWallet.Wallets;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Controls;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.NavBar;
using MagicalCryptoWallet.Fluent.ViewModels.Settings;
using NavBarView = MagicalCryptoWallet.Fluent.Views.NavBar.NavBar;

internal static class LurkingWifeModeChecks
{
	private static UiConfig _config = null!;

	public static IDisposable Run(UiContext context, string destination)
	{
		_config = new UiConfig(Path.Combine(Path.GetFullPath(destination), "synthetic-ui-config.json"));
		// Supply only the config required by the real masking controls. Wallet/network services remain unavailable.
		var services = (Services)RuntimeHelpers.GetUninitializedObject(typeof(Services));
		SetBackingField(services, nameof(Services.UiConfig), _config);
		typeof(Services).GetProperty(nameof(Services.Instance))!.SetValue(null, services);
		var repository = (WalletRepository)RuntimeHelpers.GetUninitializedObject(typeof(WalletRepository));
		SetBackingField(context, nameof(UiContext.WalletRepository), repository);

		foreach (bool initiallyEnabled in new[] { false, true })
		{
			var settings = NewSettings(initiallyEnabled);
			var mode = new LurkingWifeModeViewModel(context, settings);
			Check(mode.Title == "Lurking Wife Mode" && LurkingWifeModeViewModel.MetaData.Title == mode.Title,
				"The restored name must be used by the sidebar and navigation metadata.");
			Check(mode.LurkingWifeMode == initiallyEnabled && mode.IconName == Icon(initiallyEnabled),
				"An existing enabled/disabled setting must initialize the toggle and eye correctly.");
			mode.Toggle();
			Check(settings.PrivacyMode == !initiallyEnabled && mode.IconName == Icon(!initiallyEnabled),
				"Toggling must update the existing setting and eye.");
			settings.PrivacyMode = initiallyEnabled;
			Check(mode.LurkingWifeMode == initiallyEnabled && mode.IconName == Icon(initiallyEnabled),
				"Setting changes must also update the toggle and eye.");
		}

		var (navBar, itemModel) = CreateNavBar(context, false);
		var balance = NewMaskedText("1.23456789 BTC");
		var address = NewMaskedText("bc1qsyntheticreceiveaddress");
		var window = new Window
		{
			Width = 440, Height = 220,
			Content = new StackPanel
			{
				Orientation = Orientation.Horizontal,
				Children = { navBar, new StackPanel { Children = { balance, address } } }
			}
		};
		window.Show();
		window.Measure(new Size(440, 220));
		window.Arrange(new Rect(0, 0, 440, 220));
		try
		{
			Flush();
			var item = navBar.GetVisualDescendants().OfType<NavBarItem>().Single();
			var label = item.GetVisualDescendants().OfType<TextBlock>().Single(x => x.Text == "Lurking Wife Mode");
			Check(label.Bounds.Width <= 75 && label.TextLayout.Height <= label.Bounds.Height,
				"The full restored name must fit the existing sidebar.");
			Check(ControlAutomationPeer.CreatePeerForElement(item).GetName() == "Lurking Wife Mode"
				&& Equals(ToolTip.GetTip(item), "Lurking Wife Mode"),
				"Tooltip and accessible name must identify Lurking Wife Mode in full.");
			Check(IsRevealed(balance) && IsRevealed(address), "Disabling the mode must show balances and addresses.");

			var click = item.TranslatePoint(new Point(item.Bounds.Width / 2, item.Bounds.Height / 2), window)!.Value;
			window.MouseMove(click);
			window.MouseDown(click, MouseButton.Left);
			window.MouseUp(click, MouseButton.Left);
			Flush();
			Check(_config.PrivacyMode && itemModel.IconName == Icon(true), "The real sidebar eye toggle must enable the mode.");
			Check(!IsRevealed(balance) && !IsRevealed(address), "Enabling the mode must immediately hide balances and addresses.");

			var hover = balance.TranslatePoint(new Point(balance.Bounds.Width / 2, balance.Bounds.Height / 2), window)!.Value;
			window.MouseMove(hover);
			Flush();
			Check(!IsRevealed(balance), "Hover reveal must retain its delay.");
			WaitFor(() => IsRevealed(balance), 2, "Hover must reveal the pointed-at item after its delay.");
			Check(!IsRevealed(address), "Hover must not reveal other items.");
			WaitFor(() => !IsRevealed(balance), 12, "A hovered item must hide again after the existing timeout.");
			window.MouseMove(click);
			window.MouseMove(hover);
			WaitFor(() => IsRevealed(balance), 2, "Leaving and returning must permit another temporary reveal.");
			window.MouseMove(click);
			Flush();
			Check(!IsRevealed(balance), "Leaving a hovered item must hide it immediately.");

			balance.ForceShow = true;
			Flush();
			Check(IsRevealed(balance) && !IsRevealed(address), "Explicit reveal must affect only its own control.");
			balance.ForceShow = false;
			itemModel.OpenCommand.Execute(null);
			Flush();
			Check(!_config.PrivacyMode && IsRevealed(balance) && IsRevealed(address),
				"The sidebar command must restore visible content when the mode is disabled.");
		}
		finally
		{
			window.Close();
		}
		Console.WriteLine("Lurking Wife Mode checks passed: saved-state initialization, setting/icon synchronization, actual sidebar click, full label/tooltip/accessibility name, masking, delayed hover, automatic hiding and explicit reveal.");
		return Disposable.Empty;
	}

	public static Control CreatePreview(UiContext context, bool enabled)
	{
		var (navBar, _) = CreateNavBar(context, enabled);
		var content = new Grid { ColumnDefinitions = new ColumnDefinitions("84,*") };
		content.Children.Add(navBar);
		var details = new StackPanel
		{
			Spacing = 20, Margin = new Thickness(24), VerticalAlignment = VerticalAlignment.Center,
			Children = { NewMaskedText("1.23456789 BTC", 28), NewMaskedText("bc1qsyntheticreceiveaddress", 16) }
		};
		Grid.SetColumn(details, 1);
		content.Children.Add(details);
		return content;
	}

	private static (NavBarView, NavBarItemViewModel) CreateNavBar(UiContext context, bool enabled)
	{
		_config.PrivacyMode = enabled;
		var settings = NewSettings(enabled);
		settings.WhenAnyValue(x => x.PrivacyMode).Subscribe(value => _config.PrivacyMode = value);
		var item = new NavBarItemViewModel(context, new LurkingWifeModeViewModel(context, settings));
		// Render the actual sidebar template with one real toggle and no configured wallet.
		var model = new NavBarViewModel(context);
		model.BottomItems.Add(item);
		return (new NavBarView { DataContext = model }, item);
	}

	private static ApplicationSettings NewSettings(bool enabled)
	{
		var settings = (ApplicationSettings)RuntimeHelpers.GetUninitializedObject(typeof(ApplicationSettings));
		settings.PrivacyMode = enabled;
		return settings;
	}

	private static PrivacyContentControl NewMaskedText(string value, double size = 16) => new()
	{
		PrivacyReplacementMode = ReplacementMode.Text,
		Content = new TextBlock { Text = value, FontSize = size, Foreground = Brushes.Gray }
	};

	private static bool IsRevealed(PrivacyContentControl control) => control.GetVisualDescendants()
		.OfType<ContentPresenter>().Single(x => x.Name == "PART_ContentPresenter" && ReferenceEquals(x.TemplatedParent, control)) is { IsVisible: true, Opacity: > 0 };

	private static string Icon(bool enabled) => enabled ? "eye_hide_regular" : "eye_show_regular";
	private static void Flush() => Dispatcher.UIThread.RunJobs();
	private static void WaitFor(Func<bool> condition, double seconds, string failure)
	{
		var timer = Stopwatch.StartNew();
		while (!condition() && timer.Elapsed.TotalSeconds < seconds)
		{
			Task.Delay(20).GetAwaiter().GetResult();
			Flush();
		}
		Check(condition(), failure);
	}
	private static void SetBackingField(object target, string property, object value) => target.GetType()
		.GetField($"<{property}>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
	private static void Check(bool success, string message)
	{
		if (!success) { throw new InvalidOperationException(message); }
	}
}
