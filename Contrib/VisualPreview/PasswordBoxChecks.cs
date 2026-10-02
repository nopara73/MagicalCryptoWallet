using System.ComponentModel;
using Avalonia;
using Avalonia.Automation.Peers;
using Avalonia.Automation.Provider;
using Avalonia.Controls;
using Avalonia.Data;
using Avalonia.Headless;
using Avalonia.Input;
using Avalonia.Input.Platform;
using Avalonia.Media;
using Avalonia.Threading;
using Avalonia.VisualTree;
using MagicalCryptoWallet.Fluent.Controls;

internal static class PasswordBoxChecks
{
	public static void Run()
	{
		var model = new PasswordModel();
		var password = new CopyablePasswordTextBox
		{
			PasswordChar = '•', Watermark = "Password", Width = 400,
			FixedPasswordText = ChinesePasswordTextPresenter.CreationMaskText
		};
		password.Classes.Add("revealPasswordButton");
		password.Classes.Add("copyablePasswordTextBox");
		var notifications = new HashSet<AvaloniaProperty>();
		password.PropertyChanged += (_, e) => notifications.Add(e.Property);
		password.Bind(TextBox.TextProperty, new Binding(nameof(PasswordModel.Password)) { Source = model, Mode = BindingMode.TwoWay });
		var confirmation = new CopyablePasswordTextBox
		{
			PasswordChar = '•', FixedPasswordText = ChinesePasswordTextPresenter.CreationMaskText
		};
		var ordinary = new TextBox { Text = "Ordinary text" };
		var window = new Window
		{
			Width = 480, Height = 240,
			Content = new StackPanel { Children = { password, confirmation, ordinary } }
		};
		window.Show();
		try
		{
			Flush();
			password.Focus();
			window.KeyTextInput("abcde");
			Flush();
			Check(model.Password == "abcde", "Typing must update the bound password, never the mask.");
			Check(Rendered(password) == "This ", "The English creation sentence must replace bullets.");
			confirmation.Text = "vwxyz";
			Check(Rendered(password) == Rendered(confirmation), "Creation and confirmation must use the same mask.");

			password.CaretIndex = 2;
			window.KeyTextInput("Z");
			Check(model.Password == "abZcde", "Inserting at the caret must edit the actual password.");
			Press(Key.Back);
			Check(model.Password == "abcde", "Backspace must edit the actual password.");
			Press(Key.Delete);
			Check(model.Password == "abde", "Delete must edit the actual password.");
			password.SelectionStart = 1;
			password.SelectionEnd = 3;
			window.KeyTextInput("12");
			Check(model.Password == "a12e", "Typing must replace the selected password characters.");

			var clipboard = window.Clipboard!;
			clipboard.SetTextAsync("clipboard sentinel").GetAwaiter().GetResult();
			password.SelectAll();
			var copy = Application.Current!.PlatformSettings!.HotkeyConfiguration.Copy[0];
			Press(copy.Key, Modifiers(copy.KeyModifiers));
			Check(clipboard.TryGetTextAsync().GetAwaiter().GetResult() == "clipboard sentinel", "Hidden keyboard copy must not expose the password.");
			password.Copy();
			password.Cut();
			Flush();
			Check(clipboard.TryGetTextAsync().GetAwaiter().GetResult() == "clipboard sentinel" && model.Password == "a12e", "Hidden direct copy/cut must not expose or remove the password.");

			var peer = (IValueProvider)ControlAutomationPeer.CreatePeerForElement(password);
			Check(string.IsNullOrEmpty(peer.Value), "Accessibility must not expose a hidden password.");
			password.RevealPassword = true;
			Check(Rendered(password) == "a12e" && peer.Value == "a12e", "Reveal must show the real password.");
			Check(password.CanCopyModified && password.CanCutModified, "Reveal must enable clipboard commands for a selection.");
			Check(notifications.Contains(CopyablePasswordTextBox.CanCopyModifiedProperty), "Reveal must notify the context menu's copy binding.");
			password.Copy();
			Flush();
			Check(clipboard.TryGetTextAsync().GetAwaiter().GetResult() == "a12e", "Revealed copy must copy the password, never its mask.");
			password.RevealPassword = false;
			Check(Rendered(password) == "This" && string.IsNullOrEmpty(peer.Value), "Hiding must immediately restore the sentence and protect accessibility.");
			Check(!password.CanCopyModified && !password.CanCutModified, "Hiding must disable clipboard commands.");

			clipboard.SetTextAsync("Pasted 🪄密码").GetAwaiter().GetResult();
			password.Paste();
			Flush();
			Check(model.Password == "Pasted 🪄密码", "Paste must preserve the complete Unicode password.");
			Check(Rendered(password).Length == model.Password!.Length, "The mask must retain native UTF-16 caret indices.");
			model.Password = "e\u0301🪄";
			Flush();
			Check(password.Text == model.Password && Rendered(password).Length == model.Password.Length, "External bindings and combining characters must remain intact.");

			password.IsReadOnly = true;
			Check(!password.CanPasteModified, "Read-only state must update paste command availability.");
			Check(notifications.Contains(CopyablePasswordTextBox.CanPasteModifiedProperty), "Read-only changes must notify the context menu's paste binding.");
			password.SelectAll();
			password.Paste();
			Flush();
			Check(model.Password == "e\u0301🪄", "Read-only paste must not change the password.");
			password.IsReadOnly = false;
			Check(password.CanPasteModified, "Editable state must restore paste availability.");

			password.FixedPasswordText = null;
			model.Password = new string('x', 600);
			Flush();
			var sequence = Rendered(password);
			foreach (var phrase in new[] { "This dumb foreigner doesn't know what he's writing.", "French fries French fries French fries", "Anyone with only one chopstick will go hungry.", "Anyone who tells too many light bulb jokes will soon burn out.", "Noodle soup hot pot", "You're the cutest zombie I've ever seen.", "Never give up.", "If you were a Pokémon, I'd choose you." })
			{
				Check(sequence.Contains(phrase, StringComparison.Ordinal), "Every translated sentence must participate in the shuffled mask.");
			}
			model.Password = new string('y', 600);
			Flush();
			Check(Rendered(password) == sequence, "The mask must not depend on the password's characters.");
			model.Password = "ab";
			Flush();
			Presenter(password).PreeditText = "尚未提交";
			Check(Rendered(password) == sequence[..6] && model.Password == "ab", "IME preedit must be masked without changing the bound password.");
			Presenter(password).PreeditText = null;
			model.Password = "";
			Flush();
			Check(password.Text == "" && string.IsNullOrWhiteSpace(Rendered(password)), "Clearing the password must clear its mask.");

			Check(Rendered(ordinary) == "Ordinary text", "Ordinary text boxes must remain unchanged.");
			var presenter = Presenter(password);
			var glyphs = new Typeface(presenter.FontFamily, presenter.FontStyle, presenter.FontWeight, presenter.FontStretch).GlyphTypeface;
			foreach (char character in sequence.Distinct())
			{
				Check(glyphs.GetGlyph(character) != 0, "The application font must contain every translated sentence character.");
			}
			Console.WriteLine("English password checks passed: native editing/binding, Unicode paste, reveal, clipboard/accessibility protection, all eight translated sentences, IME and application font glyphs.");
		}
		finally
		{
			window.Close();
		}

		void Press(Key key, RawInputModifiers modifiers = RawInputModifiers.None)
		{
			window.KeyPress(key, modifiers, PhysicalKey.None, "");
			window.KeyRelease(key, modifiers, PhysicalKey.None, "");
			Flush();
		}
	}

	private static ChinesePasswordTextPresenter Presenter(Control control) => control.GetVisualDescendants().OfType<ChinesePasswordTextPresenter>().Single();
	private static string Rendered(Control control)
	{
		Flush();
		return string.Concat(Presenter(control).TextLayout.TextLines.SelectMany(line => line.TextRuns).Select(run => run.Text.ToString()));
	}
	private static void Flush() => Dispatcher.UIThread.RunJobs();
	private static void Check(bool condition, string message)
	{
		if (!condition) throw new InvalidOperationException(message);
	}
	private static RawInputModifiers Modifiers(KeyModifiers modifiers) =>
		(modifiers.HasFlag(KeyModifiers.Control) ? RawInputModifiers.Control : 0) |
		(modifiers.HasFlag(KeyModifiers.Meta) ? RawInputModifiers.Meta : 0) |
		(modifiers.HasFlag(KeyModifiers.Alt) ? RawInputModifiers.Alt : 0) |
		(modifiers.HasFlag(KeyModifiers.Shift) ? RawInputModifiers.Shift : 0);

	private sealed class PasswordModel : INotifyPropertyChanged
	{
		private string? _password = "";
		public event PropertyChangedEventHandler? PropertyChanged;
		public string? Password
		{
			get => _password;
			set { _password = value; PropertyChanged?.Invoke(this, new(nameof(Password))); }
		}
	}
}
