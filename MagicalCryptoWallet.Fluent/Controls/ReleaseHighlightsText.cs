using System;
using System.Threading;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Automation;
using Avalonia.Controls;
using Avalonia.Controls.Documents;
using Avalonia.Data;
using Avalonia.Data.Converters;
using Avalonia.Layout;
using Avalonia.Media;
using Avalonia.Threading;
using MagicalCryptoWallet.Mcw.NativeUi;

namespace MagicalCryptoWallet.Fluent.Controls;

/// <summary>Existing Avalonia text controls display Rust's typed release-note presentation.</summary>
public sealed class ReleaseHighlightsText : ScrollViewer
{
	public static readonly StyledProperty<string?> MarkdownProperty = AvaloniaProperty.Register<ReleaseHighlightsText, string?>(nameof(Markdown));
	private readonly StackPanel _panel = new();
	private CancellationTokenSource? _request;
	private bool _attached;
	private long _generation;
	internal Task LoadCompletion { get; private set; } = Task.CompletedTask;
	public event EventHandler<string>? LinkClicked;
	public bool IsLoading { get; private set; }
	public bool HasError { get; private set; }
	public string? Markdown { get => GetValue(MarkdownProperty); set => SetValue(MarkdownProperty, value); }
	static ReleaseHighlightsText() => MarkdownProperty.Changed.AddClassHandler<ReleaseHighlightsText>((control, _) => control.Refresh());
	public ReleaseHighlightsText()
	{
		Content = _panel;
		HorizontalScrollBarVisibility = Avalonia.Controls.Primitives.ScrollBarVisibility.Disabled;
		AutomationProperties.SetName(this, "Release highlights");
	}
	protected override void OnAttachedToVisualTree(VisualTreeAttachmentEventArgs e)
	{
		base.OnAttachedToVisualTree(e); _attached = true; Refresh();
	}
	protected override void OnDetachedFromVisualTree(VisualTreeAttachmentEventArgs e)
	{
		_attached = false; _generation++; _request?.Cancel(); _request?.Dispose(); _request = null;
		IsLoading = false; _panel.Children.Clear(); base.OnDetachedFromVisualTree(e);
	}
	private void Refresh()
	{
		if (!_attached) { return; }
		_request?.Cancel(); _request?.Dispose();
		_request = new CancellationTokenSource();
		var generation = ++_generation;
		HasError = false; IsLoading = true; _panel.Children.Clear();
		_panel.Children.Add(new ProgressBar { IsIndeterminate = true, Height = 2, Margin = new Thickness(0, 8) });
		LoadCompletion = LoadAsync(Markdown ?? string.Empty, generation, _request.Token);
	}
	private async Task LoadAsync(string source, long generation, CancellationToken cancellationToken)
	{
		try
		{
			var document = await MarkdownPresentation.ParseAsync(source, cancellationToken).ConfigureAwait(false);
			await Dispatcher.UIThread.InvokeAsync(() =>
			{
				if (!_attached || generation != _generation || cancellationToken.IsCancellationRequested) { return; }
				IsLoading = false; _panel.Children.Clear();
				foreach (var block in document.Blocks) { _panel.Children.Add(Render(block)); }
			});
		}
		catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested) { }
		catch (Exception)
		{
			await Dispatcher.UIThread.InvokeAsync(() =>
			{
				if (!_attached || generation != _generation) { return; }
				IsLoading = false; HasError = true; _panel.Children.Clear();
				var error = new TextBlock { Text = "Release highlights could not be loaded.", TextWrapping = TextWrapping.Wrap };
				error.Classes.Add("releaseError"); _panel.Children.Add(error);
			});
		}
	}
	private Control Render(MarkdownBlock block)
	{
		if (block.Kind == MarkdownBlockKind.Rule) { return new Border { Height = 1, Margin = new Thickness(0, 3), Classes = { "releaseRule" } }; }
		var text = new TextBlock { TextWrapping = TextWrapping.Wrap, Inlines = new InlineCollection() };
		text.Classes.Add("releaseBody");
		var multiplier = block.Kind == MarkdownBlockKind.Heading ? block.Level switch { 1 => 3.2, 2 or 3 => 1.6, 4 => 1.2, _ => 1.0 } : 1.0;
		text.Bind(TextBlock.FontSizeProperty, new Binding(nameof(FontSize)) { Source = this, Converter = new FuncValueConverter<double, double>(size => size * multiplier) });
		if (block.Kind == MarkdownBlockKind.Heading) { text.Classes.Add("releaseHeading" + block.Level); }
		for (var i = 0; i < block.Runs.Count; i++)
		{
			var run = block.Runs[i];
			if (run.Link is not { } target) { text.Inlines!.Add(Inline(run)); continue; }
			var label = new TextBlock { TextWrapping = TextWrapping.Wrap, Inlines = new InlineCollection() };
			label.Bind(TextBlock.FontSizeProperty, new Binding(nameof(TextBlock.FontSize)) { Source = text });
			label.Inlines!.Add(Inline(run));
			var labelText = run.Text;
			while (i + 1 < block.Runs.Count && block.Runs[i + 1].Link == target) { var next = block.Runs[++i]; label.Inlines.Add(Inline(next)); labelText += next.Text; }
			var button = new HyperlinkButton { Content = label, Padding = new Thickness(0), MinHeight = 0, VerticalAlignment = VerticalAlignment.Center };
			button.Classes.Add("releaseLink");
			AutomationProperties.SetName(button, labelText);
			if (run.Title is { } title) { ToolTip.SetTip(button, title); }
			button.Click += (_, _) => { if (MarkdownPresentation.IsSafeLink(target)) { LinkClicked?.Invoke(this, target); } };
			text.Inlines.Add(new InlineUIContainer { Child = button });
		}
		if (block.Kind == MarkdownBlockKind.ListItem)
		{
			var grid = new Grid { ColumnDefinitions = new ColumnDefinitions("Auto,*"), Margin = new Thickness(40 + block.Depth * 24, 0, 0, 0) };
			var marker = new TextBlock { Text = block.Marker, MinWidth = 14, Margin = new Thickness(0, 5, 5, 5) }; marker.Classes.Add("releaseMarker");
			marker.Bind(TextBlock.FontSizeProperty, new Binding(nameof(FontSize)) { Source = this });
			Grid.SetColumn(text, 1); grid.Children.Add(marker); grid.Children.Add(text); return grid;
		}
		if (block.Kind == MarkdownBlockKind.Quote) { return new Border { Child = text, BorderThickness = new Thickness(2, 0, 0, 0), Padding = new Thickness(12, 4), Margin = new Thickness(block.Depth * 8, 0, 0, 0), Classes = { "releaseQuote" } }; }
		if (block.Kind == MarkdownBlockKind.Code) { return new Border { Child = text, Padding = new Thickness(12), CornerRadius = new CornerRadius(4), Classes = { "releaseCode" } }; }
		return text;
	}
	private static Run Inline(MarkdownRun run)
	{
		var inline = new Run(run.Text);
		if ((run.Style & MarkdownStyle.Bold) != 0) { inline.FontWeight = FontWeight.Bold; }
		if ((run.Style & MarkdownStyle.Italic) != 0) { inline.FontStyle = FontStyle.Italic; }
		if ((run.Style & MarkdownStyle.Code) != 0) { inline.FontFamily = new FontFamily("Consolas, Menlo, DejaVu Sans Mono, Courier New, monospace"); }
		if ((run.Style & MarkdownStyle.Strike) != 0) { inline.TextDecorations = TextDecorations.Strikethrough; }
		return inline;
	}
}
