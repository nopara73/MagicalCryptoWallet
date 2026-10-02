using System.Diagnostics.CodeAnalysis;
using System.Reflection;
using System.Reactive.Linq;
using System.Runtime.CompilerServices;
using Avalonia;
using Avalonia.Automation.Peers;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Input;
using Avalonia.Threading;
using Avalonia.VisualTree;
using ReactiveUI;
using MagicalCryptoWallet.Fluent.ViewModels;
using MagicalCryptoWallet.Fluent.ViewModels.Settings;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Settings;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.Views.Settings;
using MagicalCryptoWallet.Fluent.Views.Wallets;
using MagicalCryptoWallet.Fluent.Views.Wallets.Settings;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoinProgressEvents;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.WabiSabi.Models;
using MagicalCryptoWallet.WabiSabi.Models.MultipartyTransaction;

internal static class CoinJoinChecks
{
	public static void Run(UiContext context)
	{
		CheckView(CreateWalletSettings(context), window =>
		{
			Check(window.GetVisualDescendants().OfType<TextBlock>().Count(text => text.Text == "Stop coinjoin threshold") == 1, "CoinJoin settings retain the balance safeguard.");
			Check(!window.GetVisualDescendants().Any(view => view is ComboBox or CheckBox or ToggleSwitch), "CoinJoin settings have no strategy or privacy switches.");
		});
		CheckView(CreateCoordinatorSettings(context), window =>
		{
			Check(window.GetVisualDescendants().OfType<TextBlock>().Count(text => text.Text is "Coordinator URI" or "Max Coinjoin Mining Fee Rate") == 2, "Coordinator settings retain connection and fee ceiling.");
			Check(!window.GetVisualDescendants().OfType<TextBlock>().Any(text => text.Text?.Contains("Input Count") == true), "Minimum input count has no editor.");
		});
		int pauses = 0, resumes = 0;
		CheckView(CreateControls(context, "waiting", () => pauses++, () => resumes++), window =>
		{
			Check(!window.GetVisualDescendants().OfType<TextBlock>().Any(text => text.Text == "Coordinator not configured" && text.IsEffectivelyVisible), "Configured controls hide the unavailable-coordinator panel.");
			var pause = window.GetVisualDescendants().OfType<Button>().Single(button => ControlAutomationPeer.CreatePeerForElement(button).GetName() == "Pause coinjoin");
			Check(pause.IsEffectivelyVisible && pause.IsEffectivelyEnabled, "Automatic CoinJoin offers an accessible pause control.");
			Click(window, pause);
		});
		CheckView(CreateControls(context, "paused", () => pauses++, () => resumes++), window =>
		{
			var play = window.GetVisualDescendants().OfType<Button>().Single(button => ControlAutomationPeer.CreatePeerForElement(button).GetName() == "Resume coinjoin");
			Check(play.IsEffectivelyVisible && play.IsEffectivelyEnabled, "Paused CoinJoin offers an accessible resume control.");
			Click(window, play);
		});
		CheckView(CreateControls(context, "signing"), window =>
		{
			Check(!window.GetVisualDescendants().OfType<Button>().Any(button => button.IsEffectivelyVisible && button.IsEffectivelyEnabled && ControlAutomationPeer.CreatePeerForElement(button).GetName() is "Pause coinjoin" or "Resume coinjoin"), "Critical phases cannot be interrupted by the controls.");
			Check(window.GetVisualDescendants().OfType<ProgressBar>().Any(bar => bar.Value == 45), "Protocol phase progress remains visible.");
		});
		Check(pauses == 1 && resumes == 1, "Pause and resume each activate exactly once.");
		CheckLatePhaseCountdown(context);
		Console.WriteLine("CoinJoin UI checks passed: retained safeguards, no strategy settings, accessible pause/resume, and protected critical-phase progress.");
	}

	public static Control CreateWalletSettings(UiContext context)
	{
		var model = NewModel<WalletCoinJoinSettingsViewModel>(context);
		model.PlebStopThreshold = "0.005";
		return new WalletCoinJoinSettingsView { DataContext = model, Margin = new Thickness(24) };
	}
	public static Control CreateCoordinatorSettings(UiContext context)
	{
		var model = NewModel<CoordinatorTabSettingsViewModel>(context);
		model.CoordinatorUri = "http://synthetic-coordinator.invalid/";
		model.MaxCoinJoinMiningFeeRate = "50";
		return new CoordinatorTabSettingsView { DataContext = model, Margin = new Thickness(24) };
	}
	[SuppressMessage("Reliability", "CA2000:Dispose objects before losing scope", Justification = "The preview view owns the commands and disposes them when detached from the visual tree.")]
	public static Control CreateControls(UiContext context, string state, Action? pause = null, Action? resume = null)
	{
		var parent = NewModel<WalletViewModel>(context);
		var model = NewModel<CoinJoinStateViewModel>(context);
		Set(parent, nameof(WalletViewModel.IsMusicBoxVisible), Observable.Return(true));
		Set(parent, nameof(WalletViewModel.WalletModel), DispatchProxy.Create<IWalletModel, CoinJoinPreviewWallet>());
		Set(parent, nameof(WalletViewModel.CoinJoinStateViewModel), model);
		model.IsCoinjoinSupported = true;
		model.PlayVisible = state == "paused";
		model.PauseVisible = state != "paused";
		model.IsInCriticalPhase = state == "signing";
		model.CurrentStatus = state switch { "paused" => "Coinjoin is paused", "signing" => "Signing coinjoin", "private" => "All coins are private", _ => "Awaiting a round" };
		model.ProgressValue = state == "signing" ? 45 : state == "private" ? 100 : 0;
		model.LeftText = state == "signing" ? "00:00:15" : "";
		model.RightText = state == "signing" ? "-00:00:18" : "";
		var pauseCommand = ReactiveCommand.Create(() => pause?.Invoke(), Observable.Return(state != "signing"));
		var playCommand = ReactiveCommand.Create(() => resume?.Invoke());
		var inertCommand = ReactiveCommand.Create(() => { });
		Set(model, nameof(CoinJoinStateViewModel.StopPauseCommand), pauseCommand);
		Set(model, nameof(CoinJoinStateViewModel.PlayCommand), playCommand);
		Set(model, nameof(CoinJoinStateViewModel.CanNavigateToCoinjoinSettings), Observable.Return(true));
		Set(model, nameof(CoinJoinStateViewModel.NavigateToSettingsCommand), inertCommand);
		Set(model, nameof(CoinJoinStateViewModel.NavigateToCoordinatorSettingsCommand), inertCommand);
		Set(model, nameof(CoinJoinStateViewModel.CoinJoinPaymentsCommand), inertCommand);
		var view = new MusicControlsView { DataContext = parent };
		view.DetachedFromVisualTree += (_, _) => { pauseCommand.Dispose(); playCommand.Dispose(); inertCommand.Dispose(); };
		return view;
	}
	private static T NewModel<T>(UiContext context) where T : ViewModelBase
	{
		var model = (T)RuntimeHelpers.GetUninitializedObject(typeof(T));
		typeof(ViewModelBase).GetConstructor([typeof(UiContext)])!.Invoke(model, [context]);
		return model;
	}
	private static void CheckLatePhaseCountdown(UiContext context)
	{
		var parameters = (RoundParameters)RuntimeHelpers.GetUninitializedObject(typeof(RoundParameters));
		Set(parameters, nameof(RoundParameters.OutputRegistrationTimeout), TimeSpan.FromMinutes(1));
		Set(parameters, nameof(RoundParameters.TransactionSigningTimeout), TimeSpan.FromMinutes(1));
		var round = (RoundState)RuntimeHelpers.GetUninitializedObject(typeof(RoundState));
		Set(round, nameof(RoundState.CoinjoinState), new ConstructionState(parameters));
		var method = typeof(CoinJoinStateViewModel).GetMethod("OnCoinJoinPhaseChanged", BindingFlags.NonPublic | BindingFlags.Instance)!;
		foreach (bool signing in new[] { false, true })
		{
			var model = NewModel<CoinJoinStateViewModel>(context);
			var timer = new DispatcherTimer { Interval = TimeSpan.FromSeconds(1) };
			typeof(CoinJoinStateViewModel).GetField("_countdownTimer", BindingFlags.NonPublic | BindingFlags.Instance)!.SetValue(model, timer);
			CoinJoinProgressEventArgs phase = signing
				? new EnteringSigningPhase(round, DateTimeOffset.UtcNow.AddSeconds(30))
				: new EnteringOutputRegistrationPhase(round, DateTimeOffset.UtcNow.AddSeconds(30));
			try
			{
				method.Invoke(model, [phase]);
				Check(timer.IsEnabled && model.CurrentStatus == "Coinjoin in progress" && model.LeftText.Length > 0 && model.ProgressValue is > 0 and < 100,
					"Opening the wallet during output registration or signing initializes accurate phase progress.");
			}
			finally { timer.Stop(); }
		}
	}
	private static void Set(object target, string name, object value) => target.GetType().GetField($"<{name}>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
	private static void CheckView(Control view, Action<Window> assertion)
	{
		var window = new Window { Width = 800, Height = 450, Content = view };
		window.Show(); Flush(); window.Measure(new Size(800, 450)); window.Arrange(new Rect(0, 0, 800, 450));
		try { Flush(); assertion(window); } finally { window.Close(); }
	}
	private static void Click(Window window, Control control)
	{
		var point = control.TranslatePoint(new Point(control.Bounds.Width / 2, control.Bounds.Height / 2), window)!.Value;
		window.MouseMove(point); window.MouseDown(point, MouseButton.Left); window.MouseUp(point, MouseButton.Left); Flush();
	}
	private static void Flush() { Dispatcher.UIThread.RunJobs(); AvaloniaHeadlessPlatform.ForceRenderTimerTick(); Dispatcher.UIThread.RunJobs(); }
	private static void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
}

public class CoinJoinPreviewWallet : DispatchProxy
{
	protected override object? Invoke(MethodInfo? method, object?[]? args) => method?.Name switch
	{
		"get_IsCoinJoinEnabled" => true,
		"add_PropertyChanged" or "remove_PropertyChanged" => null,
		_ => throw new InvalidOperationException("Wallet services are unavailable in the CoinJoin visual preview.")
	};
}
