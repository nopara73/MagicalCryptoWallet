using System.Reflection;
using System.Diagnostics.CodeAnalysis;
using System.Runtime.CompilerServices;
using Avalonia;
using Avalonia.Automation.Peers;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Input;
using Avalonia.Layout;
using Avalonia.Threading;
using Avalonia.VisualTree;
using ReactiveUI;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Controls;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels;
using MagicalCryptoWallet.Fluent.ViewModels.AddWallet;
using MagicalCryptoWallet.Fluent.ViewModels.Login;
using MagicalCryptoWallet.Fluent.ViewModels.NavBar;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets;
using MagicalCryptoWallet.Fluent.Views.Login;
using NavBarView = MagicalCryptoWallet.Fluent.Views.NavBar.NavBar;

internal static class SingleWalletChecks
{
	public static void Run(UiContext context)
	{
		Check(AddWalletPageViewModel.MetaData is { Searchable: false, NavBarPosition: NavBarPosition.None },
			"Initial wallet setup must not appear in the sidebar or action search.");
		int opened = 0;
		var content = CreatePreview(context, () => opened++);
		var window = new Window { Width = 800, Height = 600, Content = content };
		window.Show();
		Dispatcher.UIThread.RunJobs();
		window.Measure(new Size(800, 600));
		window.Arrange(new Rect(0, 0, 800, 600));
		try
		{
			var sidebar = content.GetVisualDescendants().OfType<NavBarView>().Single();
			Check(!sidebar.GetVisualDescendants().OfType<ListBox>().Any(), "The sidebar must have no wallet selector.");
			var home = sidebar.GetVisualDescendants().OfType<NavBarItem>().Single();
			var accessibleName = ControlAutomationPeer.CreatePeerForElement(home).GetName();
			Check(home.Bounds.Width == 75 && accessibleName == "My Wallet",
				$"The one wallet home button must retain its size and accessible name: width={home.Bounds.Width}, name={accessibleName}, context={home.DataContext?.GetType().Name}, title={(home.DataContext as WalletPageViewModel)?.Title}, tooltip={ToolTip.GetTip(home)}, command={home.Command is not null}.");
			var point = home.TranslatePoint(new Point(home.Bounds.Width / 2, home.Bounds.Height / 2), window)!.Value;
			window.MouseMove(point);
			window.MouseDown(point, MouseButton.Left);
			window.MouseUp(point, MouseButton.Left);
			Dispatcher.UIThread.RunJobs();
			Check(opened == 1, "The real home button must execute its command exactly once.");
			home.Focus();
			window.KeyPress(Key.Enter, RawInputModifiers.None, PhysicalKey.Enter, "");
			window.KeyRelease(Key.Enter, RawInputModifiers.None, PhysicalKey.Enter, "");
			window.KeyPress(Key.Space, RawInputModifiers.None, PhysicalKey.Space, " ");
			window.KeyRelease(Key.Space, RawInputModifiers.None, PhysicalKey.Space, " ");
			Dispatcher.UIThread.RunJobs();
			Check(opened == 3, "The home button must remain operable with Enter and Space after removing the wallet list.");
		}
		finally
		{
			window.Close();
		}
		Console.WriteLine("Single-wallet UI checks passed: first-run setup excluded from navigation/search, no wallet selector, one accessible home button and actual mouse/Enter/Space activation.");
	}

	[SuppressMessage("Reliability", "CA2000:Dispose objects before losing scope", Justification = "The returned sidebar owns its command and disposes it on detachment.")]
	public static Control CreatePreview(UiContext context, Action? onOpen = null)
	{
		// Render the real sidebar and login views without initializing wallet or network services.
		var page = (WalletPageViewModel)RuntimeHelpers.GetUninitializedObject(typeof(WalletPageViewModel));
		// Initialize base binding/validation events while leaving wallet and network services inert.
		typeof(ViewModelBase).GetConstructor([typeof(UiContext)])!.Invoke(page, [context]);
		page.Title = "My Wallet";
		page.IconName = "nav_wallet_24_regular";
		page.IconNameFocused = "nav_wallet_24_filled";
		page.IsSelected = true;
		var command = ReactiveCommand.Create(() => onOpen?.Invoke());
		SetBackingField(page, nameof(WalletPageViewModel.OpenCommand), command);
		var model = new NavBarViewModel(context) { Wallet = page };
		var sidebar = new NavBarView { DataContext = model };
		sidebar.DetachedFromVisualTree += (_, _) => command.Dispose();
		var wallet = DispatchProxy.Create<IWalletModel, SinglePreviewWallet>();
		var login = new LoginView { DataContext = new LoginViewModel(context, wallet) { Password = "synthetic-passphrase" } };
		var content = new Grid { ColumnDefinitions = new ColumnDefinitions("84,*") };
		content.Children.Add(sidebar);
		Grid.SetColumn(login, 1);
		content.Children.Add(login);
		return content;
	}

	private static void SetBackingField(object target, string property, object value) => target.GetType()
		.GetField($"<{property}>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
	private static void Check(bool success, string message)
	{
		if (!success) { throw new InvalidOperationException(message); }
	}
}

public class SinglePreviewWallet : DispatchProxy
{
	private readonly WalletSettingsModel _settings = (WalletSettingsModel)RuntimeHelpers.GetUninitializedObject(typeof(WalletSettingsModel));
	protected override object? Invoke(MethodInfo? targetMethod, object?[]? args) => targetMethod?.Name switch
	{
		"get_Name" => "My Wallet",
		"get_IsWatchOnlyWallet" => false,
		"get_Settings" => _settings,
		_ => throw new InvalidOperationException("Wallet services are unavailable in this preview.")
	};
}
