using System.Collections.Generic;
using Avalonia.Controls.Presenters;
using Avalonia.Media;
using Avalonia.Media.TextFormatting;
using Avalonia.Utilities;

namespace MagicalCryptoWallet.Fluent.Controls;

/// <summary>
/// Restores the sentence mask from the original 2018/2019 password box.
/// Only the rendered layout changes; TextBox retains the real password and native editing.
/// </summary>
public class ChinesePasswordTextPresenter : TextPresenter
{
	public const string CreationMaskText = "这个笨老外不知道自己在写什么。";

	private static readonly string[] Sentences =
	[
		CreationMaskText,
		"法式炸薯条法式炸薯条法式炸薯条",
		"只有一支筷子的人会挨饿。",
		"说太多灯泡笑话的人，很快就会心力交瘁。",
		"汤面火锅",
		"你是我见过的最可爱的僵尸。",
		"永不放弃。",
		"如果你是只宠物小精灵，我就选你。"
	];

	private static readonly FontFamily MaskFont =
		new("avares://MagicalCryptoWallet.Fluent/Assets/Fonts#MagicalCryptoWallet Password Mask");

	private string? _sequence;

	protected override Type StyleKeyOverride => typeof(TextPresenter);

	internal void ResetMask()
	{
		_sequence = null;
		InvalidateTextLayout();
	}

	protected override TextLayout CreateTextLayout()
	{
		if (TemplatedParent is not CopyablePasswordTextBox owner || PasswordChar == default || RevealPassword)
		{
			return base.CreateTextLayout();
		}

		if (_sequence is null)
		{
			var sentences = (string[])Sentences.Clone();
			Random.Shared.Shuffle(sentences);
			_sequence = string.Concat(sentences);
		}

		var sequence = string.IsNullOrEmpty(owner.FixedPasswordText) ? _sequence : owner.FixedPasswordText;
		var length = (Text?.Length ?? 0) + (PreeditText?.Length ?? 0);
		var mask = string.Create(length, sequence, static (characters, phrase) =>
		{
			for (var i = 0; i < characters.Length; i++)
			{
				characters[i] = phrase[i % phrase.Length];
			}
		});

		var typeface = new Typeface(MaskFont);
		IReadOnlyList<ValueSpan<TextRunProperties>>? overrides = null;
		if (!string.IsNullOrEmpty(PreeditText))
		{
			overrides = [new(CaretIndex, PreeditText.Length,
				new GenericTextRunProperties(typeface, FontFeatures, FontSize,
					foregroundBrush: Foreground, textDecorations: TextDecorations.Underline))];
		}
		else if (ShowSelectionHighlight && SelectionStart != SelectionEnd && SelectionForegroundBrush is not null)
		{
			overrides = [new(Math.Min(SelectionStart, SelectionEnd), Math.Abs(SelectionEnd - SelectionStart),
				new GenericTextRunProperties(typeface, FontFeatures, FontSize, foregroundBrush: SelectionForegroundBrush))];
		}

		// Let the native presenter determine its exact scrolling and layout constraints.
		// Its temporary layout is password-masked too, never a layout of the hidden secret.
		using var nativeLayout = base.CreateTextLayout();
		return new TextLayout(mask, typeface, FontFeatures, FontSize, Foreground, TextAlignment, TextWrapping,
			maxWidth: nativeLayout.MaxWidth, maxHeight: nativeLayout.MaxHeight, textStyleOverrides: overrides,
			flowDirection: FlowDirection, lineHeight: LineHeight, letterSpacing: LetterSpacing);
	}
}
