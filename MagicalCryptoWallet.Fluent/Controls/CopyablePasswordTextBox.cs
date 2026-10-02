using System.Collections.Generic;
using System.Linq;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Automation.Peers;
using Avalonia.Automation.Provider;
using Avalonia.Controls;
using Avalonia.Controls.Primitives;
using Avalonia.Input;
using Avalonia.Interactivity;

namespace MagicalCryptoWallet.Fluent.Controls;

public partial class CopyablePasswordTextBox : TextBox
{
	public static readonly StyledProperty<string?> FixedPasswordTextProperty =
		AvaloniaProperty.Register<CopyablePasswordTextBox, string?>(nameof(FixedPasswordText));

	public static readonly DirectProperty<CopyablePasswordTextBox, bool> CanCutModifiedProperty =
		AvaloniaProperty.RegisterDirect<CopyablePasswordTextBox, bool>(
			nameof(CanCutModified),
			o => o.CanCutModified);

	public static readonly DirectProperty<CopyablePasswordTextBox, bool> CanCopyModifiedProperty =
		AvaloniaProperty.RegisterDirect<CopyablePasswordTextBox, bool>(
			nameof(CanCopyModified),
			o => o.CanCopyModified);

	public static readonly DirectProperty<CopyablePasswordTextBox, bool> CanPasteModifiedProperty =
		AvaloniaProperty.RegisterDirect<CopyablePasswordTextBox, bool>(
			nameof(CanPasteModified),
			o => o.CanPasteModified);

	private bool _canCutModified;
	private bool _canCopyModified;
	private bool _canPasteModified;
	private ChinesePasswordTextPresenter? _passwordPresenter;

	public CopyablePasswordTextBox()
	{
		CopyingToClipboard += (_, e) => e.Handled |= !RevealPassword;
		CuttingToClipboard += (_, e) => e.Handled |= !RevealPassword || IsReadOnly;
		PastingFromClipboard += (_, e) => e.Handled |= IsReadOnly;
	}

	protected override Type StyleKeyOverride => typeof(TextBox);

	public string? FixedPasswordText
	{
		get => GetValue(FixedPasswordTextProperty);
		set => SetValue(FixedPasswordTextProperty, value);
	}

	public bool CanCutModified
	{
		get => _canCutModified;
		private set => SetAndRaise(CanCutModifiedProperty, ref _canCutModified, value);
	}

	public bool CanCopyModified
	{
		get => _canCopyModified;
		private set => SetAndRaise(CanCopyModifiedProperty, ref _canCopyModified, value);
	}

	public bool CanPasteModified
	{
		get => _canPasteModified;
		private set => SetAndRaise(CanPasteModifiedProperty, ref _canPasteModified, value);
	}

	private string GetSelection()
	{
		var text = Text;

		if (string.IsNullOrEmpty(text))
		{
			return "";
		}

		var selectionStart = SelectionStart;
		var selectionEnd = SelectionEnd;
		var start = Math.Min(selectionStart, selectionEnd);
		var end = Math.Max(selectionStart, selectionEnd);

		if (start == end || (Text?.Length ?? 0) < end)
		{
			return "";
		}

		return text[start..end];
	}

	private void UpdateCommandStates()
	{
		var text = GetSelection();
		var isSelectionNullOrEmpty = string.IsNullOrEmpty(text);
		CanCopyModified = RevealPassword && !isSelectionNullOrEmpty;
		CanCutModified = RevealPassword && !isSelectionNullOrEmpty && !IsReadOnly;
		CanPasteModified = !IsReadOnly;
	}

	protected override void OnKeyDown(KeyEventArgs e)
	{
		var handled = false;
		var keymap = Application.Current!.PlatformSettings!.HotkeyConfiguration;

		bool Match(List<KeyGesture> gestures) => gestures.Any(g => g.Matches(e));

		if (Match(keymap.Copy))
		{
			Copy();
			handled = true;
		}
		else if (Match(keymap.Cut))
		{
			Cut();
			handled = true;
		}
		else if (Match(keymap.Paste))
		{
			Paste();
			handled = true;
		}

		if (handled)
		{
			e.Handled = true;
		}
		else
		{
			base.OnKeyDown(e);
		}
	}

	protected override void OnPropertyChanged(AvaloniaPropertyChangedEventArgs change)
	{
		base.OnPropertyChanged(change);

		if (change.Property == TextProperty)
		{
			UpdateCommandStates();
		}
		else if (change.Property == SelectionStartProperty)
		{
			UpdateCommandStates();
		}
		else if (change.Property == SelectionEndProperty)
		{
			UpdateCommandStates();
		}
		else if (change.Property == RevealPasswordProperty)
		{
			UpdateCommandStates();
		}
		else if (change.Property == IsReadOnlyProperty)
		{
			UpdateCommandStates();
		}
		else if (change.Property == FixedPasswordTextProperty)
		{
			_passwordPresenter?.ResetMask();
		}
	}

	protected override void OnGotFocus(GotFocusEventArgs e)
	{
		base.OnGotFocus(e);

		if (string.IsNullOrEmpty(Text))
		{
			_passwordPresenter?.ResetMask();
		}

		UpdateCommandStates();
	}

	protected override void OnLostFocus(RoutedEventArgs e)
	{
		base.OnLostFocus(e);

		UpdateCommandStates();
	}

	protected override void OnApplyTemplate(TemplateAppliedEventArgs e)
	{
		base.OnApplyTemplate(e);
		_passwordPresenter = e.NameScope.Find<ChinesePasswordTextPresenter>("PART_TextPresenter");
		UpdateCommandStates();
	}

	protected override AutomationPeer OnCreateAutomationPeer() => new PasswordAutomationPeer(this);

	private sealed class PasswordAutomationPeer(CopyablePasswordTextBox owner) : ControlAutomationPeer(owner), IValueProvider
	{
		public bool IsReadOnly => owner.IsReadOnly;
		public string? Value => owner.RevealPassword ? owner.Text : string.Empty;

		public void SetValue(string? value)
		{
			if (!owner.IsEffectivelyEnabled || IsReadOnly)
			{
				throw new InvalidOperationException("The passphrase field cannot be edited.");
			}
			owner.SetCurrentValue(TextProperty, value);
		}

		protected override AutomationControlType GetAutomationControlTypeCore() => AutomationControlType.Edit;
		protected override string? GetPlaceholderTextCore() => owner.Watermark;
	}
}
