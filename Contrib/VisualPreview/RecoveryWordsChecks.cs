using System.Reflection;
using System.Runtime.CompilerServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Headless;
using Avalonia.Input;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Styling;
using Avalonia.Threading;
using Avalonia.VisualTree;
using NBitcoin;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Models;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.AddWallet.Create;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Fluent.Views.AddWallet.Create;
using MagicalCryptoWallet.Wallets;

internal static class RecoveryWordsChecks
{
	public static void Run(string destination)
	{
		int visits = 0;
		foreach (var theme in new[] { ThemeVariant.Light, ThemeVariant.Dark })
		foreach (int wordCount in new[] { 12, 24 })
		foreach (int width in new[] { 640, 800 })
		{
			Application.Current!.RequestedThemeVariant = theme;
			var context = CreateInertContext(destination);
			var dialog = new TargettedNavigationStack(context, NavigationTarget.DialogScreen);
			var compactDialog = new TargettedNavigationStack(context, NavigationTarget.CompactDialogScreen);
			context.RegisterNavigation(new NavigationState(context,
				new TargettedNavigationStack(context, NavigationTarget.HomeScreen), dialog,
				new TargettedNavigationStack(context, NavigationTarget.FullScreen), compactDialog, null!));
			var entropy = wordCount == 12 ? new byte[16] : Enumerable.Range(0, 32).Select(x => (byte)x).ToArray();
			var mnemonic = new Mnemonic(Wordlist.English, entropy);
			var options = new WalletCreationOptions.AddNewWallet(new RecoveryWordsBackup("", mnemonic));
			var recovery = new RecoveryWordsViewModel(context, options);
			dialog.To(recovery);
			var window = new Window
			{
				Width = width, Height = 600,
				Background = theme == ThemeVariant.Dark ? new SolidColorBrush(Color.Parse("#151515")) : Brushes.White,
				Content = new RecoveryWordsView { DataContext = recovery }
			};
			window.Show();
			try
			{
				Flush(window);
				Check(recovery.MnemonicWords.Count == wordCount, "The complete numbered backup must still be displayed.");
				Click(window, Button(window, "PART_NextButton"));
				var confirmation = dialog.CurrentPage as ConfirmRecoveryWordsViewModel
					?? throw new InvalidOperationException("Recovery Continue did not open the real confirmation page.");
				window.Content = new ConfirmRecoveryWordsView { DataContext = confirmation };
				Flush(window);
				CheckFreshVisit(window, confirmation, wordCount);
				Capture("initial");

				var wrong = Choices(window).First(x => ((RecoveryWordViewModel)x.DataContext!).Word != confirmation.CurrentWord.Word);
				Click(window, wrong);
				Check(!Button(window, "PART_NextButton").IsEffectivelyEnabled && !confirmation.CurrentWord.IsConfirmed,
					"Clicking a wrong word must keep Continue disabled.");
				Check(Choices(window).Count(x => x.IsEffectivelyEnabled) == 1 && wrong.IsEffectivelyEnabled,
					"The wrong choice must remain clickable so the user can clear it.");
				Capture("wrong");
				Click(window, wrong);
				Check(confirmation.CurrentWord.SelectedWord is null, "Deselecting a wrong word must clear it.");
				AnswerFour(window, confirmation);
				Capture("confirmed");

				Click(window, Button(window, "PART_BackButton"));
				Check(ReferenceEquals(dialog.CurrentPage, recovery), "Back must return to the complete backup page.");
				Check(recovery.MnemonicWords.Select(x => x.Word).SequenceEqual(mnemonic.Words)
					&& recovery.MnemonicWords.All(x => !x.IsConfirmed && x.SelectedWord is null),
					"Confirmation must not alter any recovery material or its display state.");
				window.Content = new RecoveryWordsView { DataContext = recovery };
				Flush(window);
				Click(window, Button(window, "PART_NextButton"));
				var freshConfirmation = dialog.CurrentPage as ConfirmRecoveryWordsViewModel
					?? throw new InvalidOperationException("Re-entering confirmation did not open the real page.");
				Check(!ReferenceEquals(confirmation, freshConfirmation), "Proceeding again must create a fresh confirmation visit.");
				window.Content = new ConfirmRecoveryWordsView { DataContext = freshConfirmation };
				Flush(window);
				CheckFreshVisit(window, freshConfirmation, wordCount);
				Capture("reentered");
				AnswerFour(window, freshConfirmation);
				Click(window, Button(window, "PART_NextButton"));
				Check(compactDialog.CurrentPage is CreatePasswordDialogViewModel,
					"After four correct answers, Continue must open the existing password dialog.");
				// Cancel before wallet creation. All wallet/key/network services remain inert.
				compactDialog.CurrentPage!.CancelCommand.Execute(null);
				Flush(window);
				Check(!context.WalletSetupService.HasWallet, "Synthetic UI verification must never create a wallet.");
				visits += 2;
			}
			finally
			{
				window.Close();
				dialog.Clear();
				compactDialog.Clear();
			}

			void Capture(string state)
			{
				if (width != 800) { return; }
				foreach (double scale in new[] { 1.0, 2.0 })
				{
					using var bitmap = new RenderTargetBitmap(new PixelSize((int)(width * scale), (int)(600 * scale)), new Vector(96 * scale, 96 * scale));
					bitmap.Render(window);
					bitmap.Save(Path.Combine(destination, $"recovery-{wordCount}-{state}-{theme.Key!.ToString()!.ToLowerInvariant()}-{(int)(scale * 100)}.png"));
				}
			}
		}
		Console.WriteLine($"Recovery confirmation UI passed: {visits} visits, real numbered prompts and full 12/24-word pools, wrong-answer blocking/correction, four-answer completion, Back/re-entry resets and password handoff; light/dark, 640/800px, captures at 100/200 percent. No wallet was created.");
	}

	private static void CheckFreshVisit(Window window, ConfirmRecoveryWordsViewModel model, int wordCount)
	{
		Check(model.ConfirmationWords.Count == 4 && model.ConfirmationWords.Select(x => x.Index).Distinct().Count() == 4
			&& model.ConfirmationWords.All(x => x.Index >= 1 && x.Index <= wordCount), "The page must check four distinct valid mnemonic positions.");
		Check(model.AvailableWords.Count == wordCount && Choices(window).Count() == wordCount,
			"All mnemonic word choices, including duplicates and distractors, must be rendered.");
		Check(!model.AllWordsConfirmed && !Button(window, "PART_NextButton").IsEffectivelyEnabled
			&& model.ConfirmationWords.All(x => x.SelectedWord is null && !x.IsConfirmed)
			&& model.AvailableWords.All(x => !x.IsSelected && !x.IsConfirmed),
			$"A fresh visit must discard all previous answers and selections: allConfirmed={model.AllWordsConfirmed}, buttonEnabled={Button(window, "PART_NextButton").IsEffectivelyEnabled}, commandEnabled={model.NextCommand!.CanExecute(null)}, commandBound={ReferenceEquals(Button(window, "PART_NextButton").Command, model.NextCommand)}, answered={model.ConfirmationWords.Count(x => x.SelectedWord is not null)}, selectedChoices={model.AvailableWords.Count(x => x.IsSelected)}, consumedChoices={model.AvailableWords.Count(x => x.IsConfirmed)}.");
		foreach (var prompt in model.ConfirmationWords)
		{
			var number = window.GetVisualDescendants().OfType<TextBlock>().Single(x => ReferenceEquals(x.DataContext, prompt) && x.Text == $"{prompt.Index}.");
			CheckFits(window, number);
		}
		foreach (var choice in Choices(window)) { CheckFits(window, choice); }
	}

	private static void AnswerFour(Window window, ConfirmRecoveryWordsViewModel model)
	{
		for (int i = 0; i < 4; i++)
		{
			var choice = Choices(window).First(x => x.IsEffectivelyEnabled && ((RecoveryWordViewModel)x.DataContext!).Word == model.CurrentWord.Word);
			Click(window, choice);
			Check(model.ConfirmationWords.Count(x => x.IsConfirmed) == i + 1,
				"Each correct click must confirm exactly one original word position, including repeated values.");
			Check(Button(window, "PART_NextButton").IsEffectivelyEnabled == (i == 3), "Continue must only enable after the fourth correct answer.");
		}
		foreach (var prompt in model.ConfirmationWords)
		{
			var answer = window.GetVisualDescendants().OfType<TextBlock>().Single(x => ReferenceEquals(x.DataContext, prompt) && x.Text == prompt.Word);
			CheckFits(window, answer);
			Check(answer.TextLayout.Height <= answer.Bounds.Height && answer.TextLayout.Width <= answer.Bounds.Width,
				"The completed word must fit without text clipping.");
		}
	}

	private static IEnumerable<ToggleButton> Choices(Window window) => window.GetVisualDescendants().OfType<ToggleButton>()
		.Where(x => x.DataContext is RecoveryWordViewModel);
	private static Button Button(Window window, string name) => window.GetVisualDescendants().OfType<Button>().Single(x => x.Name == name);
	private static void Click(Window window, Control control)
	{
		Check(control.IsEffectivelyEnabled, "The actual control must be enabled before clicking.");
		var point = control.TranslatePoint(new Point(control.Bounds.Width / 2, control.Bounds.Height / 2), window)!.Value;
		window.MouseMove(point);
		window.MouseDown(point, MouseButton.Left);
		window.MouseUp(point, MouseButton.Left);
		Flush(window);
	}
	private static void Flush(Window window)
	{
		Dispatcher.UIThread.RunJobs();
		window.Measure(new Size(window.Width, window.Height));
		window.Arrange(new Rect(0, 0, window.Width, window.Height));
		AvaloniaHeadlessPlatform.ForceRenderTimerTick();
		Dispatcher.UIThread.RunJobs();
	}
	private static void CheckFits(Window window, Control control)
	{
		var position = control.TranslatePoint(default, window)!.Value;
		Check(control.Bounds.Width > 0 && control.Bounds.Height > 0 && position.X >= 0 && position.Y >= 0
			&& position.X + control.Bounds.Width <= window.Bounds.Width + 1 && position.Y + control.Bounds.Height <= window.Bounds.Height + 1,
			"All prompt numbers, answers and word choices must fit inside the window.");
	}
	private static UiContext CreateInertContext(string destination)
	{
		var session = (WalletSession)RuntimeHelpers.GetUninitializedObject(typeof(WalletSession));
		typeof(WalletSession).GetField("_gate", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(session, Activator.CreateInstance(typeof(System.Threading.Lock)));
		SetProperty(session, nameof(WalletSession.Network), Network.RegTest);
		var services = (Services)RuntimeHelpers.GetUninitializedObject(typeof(Services));
		SetProperty(services, nameof(Services.WalletSession), session);
		SetProperty(services, nameof(Services.UiConfig), new UiConfig(Path.Combine(Path.GetFullPath(destination), "synthetic-recovery-ui-config.json")));
		typeof(Services).GetProperty(nameof(Services.Instance))!.SetValue(null, services);
		var setup = (WalletSetupService)RuntimeHelpers.GetUninitializedObject(typeof(WalletSetupService));
		typeof(WalletSetupService).GetField("_services", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(setup, services);
		var context = (UiContext)RuntimeHelpers.GetUninitializedObject(typeof(UiContext));
		SetProperty(context, nameof(UiContext.Services), services);
		SetProperty(context, nameof(UiContext.WalletSetupService), setup);
		return context;
	}
	private static void SetProperty(object target, string property, object value) => target.GetType()
		.GetField($"<{property}>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
	private static void Check(bool passed, string message)
	{
		if (!passed) { throw new InvalidOperationException(message); }
	}
}
