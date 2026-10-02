using System.Buffers.Binary;
using System.Reflection;
using System.Text;
using System.Xml.Linq;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Controls.Documents;
using Avalonia.Headless;
using Avalonia.Input;
using Avalonia.Markup.Xaml.Styling;
using Avalonia.Media;
using Avalonia.Styling;
using Avalonia.Themes.Fluent;
using Avalonia.Threading;
using Avalonia.VisualTree;
using MagicalCryptoWallet.Fluent.Controls;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.NativeUi;

// Test-only fixture responses exercise the actual managed boundary and controls.
// A separate --wire pass consumes real Rust dispatch output; no parser exists here.
var output = args.Length > 0 ? Path.GetFullPath(args[0]) : throw new ArgumentException("Output directory required.");
Directory.CreateDirectory(output);
AppBuilder.Configure<Application>().WithInterFont().With(new FontManagerOptions { DefaultFamilyName = "fonts:Inter#Inter, $Default" })
    .UseSkia().UseHeadless(new AvaloniaHeadlessPlatformOptions { UseHeadlessDrawing = false }).SetupWithoutStarting();
Application.Current!.Styles.Add(new FluentTheme());
Application.Current.Styles.Add(new StyleInclude(new Uri("avares://NativeUiVerification/")) { Source = new Uri("avares://NativeUiVerification/Styles/ReleaseHighlights.axaml") });
var checks = new List<string>();
var service = new FixtureService();
using var registration = McwApplicationServices.Bind(service);
var specimen = Document(
    new(MarkdownBlockKind.Heading, 2, 0, "", [new("Summary", MarkdownStyle.None, null, null)]),
    new(MarkdownBlockKind.Heading, 4, 0, "", [new("🦀 Release highlights", MarkdownStyle.None, null, null)]),
    new(MarkdownBlockKind.Paragraph, 0, 0, "", [new("Readable ", MarkdownStyle.None, null, null), new("bold", MarkdownStyle.Bold, null, null), new(" and ", MarkdownStyle.None, null, null), new("italic", MarkdownStyle.Italic, null, null), new(" text with café and 你好.", MarkdownStyle.None, null, null)]),
    new(MarkdownBlockKind.ListItem, 0, 0, "•", [new("A long list item wraps inside the existing content area at narrow widths, keeping its marker aligned with the first line.", MarkdownStyle.None, null, null)]),
    new(MarkdownBlockKind.ListItem, 0, 1, "3.", [new("A nested numbered item", MarkdownStyle.None, null, null)]),
    new(MarkdownBlockKind.Paragraph, 0, 0, "", [new("Read the ", MarkdownStyle.None, null, null), new("release", MarkdownStyle.Bold, "https://example.test/release", "Release details"), new(" notes", MarkdownStyle.Italic, "https://example.test/release", "Release details"), new(".", MarkdownStyle.None, null, null)]),
    new(MarkdownBlockKind.Code, 0, 0, "", [new("mcw --help\nplain code <&>", MarkdownStyle.Code, null, null)]),
    new(MarkdownBlockKind.Quote, 0, 1, "", [new("A quoted line", MarkdownStyle.None, null, null)]),
    new(MarkdownBlockKind.Rule, 0, 0, "", []));
service.Complete(specimen);
var decoded = MarkdownPresentation.Decode(specimen);
Check(decoded.Blocks.Count == 9 && decoded.Blocks[2].Runs[1].Style == MarkdownStyle.Bold, "typed fixture decode");

foreach (var length in Enumerable.Range(0, specimen.Length))
{
    ExpectInvalid(() => MarkdownPresentation.Decode(specimen.AsSpan(0, length)));
}
ExpectInvalid(() => MarkdownPresentation.Decode(specimen.Concat(new byte[] { 0 }).ToArray()));
ExpectInvalid(() => MarkdownPresentation.Decode(new byte[] { 2, 0, 0, 0, 0 }));
ExpectInvalid(() => MarkdownPresentation.Decode(new byte[] { 1, 1, 0, 0, 0, 99, 0, 0 }));
var invalidUtf8 = Document(new MarkdownBlock(MarkdownBlockKind.Paragraph, 0, 0, "", [new("x", MarkdownStyle.None, null, null)]));
invalidUtf8[21] = 0xff;
ExpectInvalid(() => MarkdownPresentation.Decode(invalidUtf8));
ExpectInvalid(() => MarkdownPresentation.Decode(Document(new MarkdownBlock(MarkdownBlockKind.Paragraph, 0, 0, "", [new("x", (MarkdownStyle)16, null, null)]))));
ExpectInvalid(() => MarkdownPresentation.Decode(Document(new MarkdownBlock(MarkdownBlockKind.Heading, 7, 0, "", []))));
ExpectInvalid(() => MarkdownPresentation.Decode(Document(new MarkdownBlock(MarkdownBlockKind.Paragraph, 0, 0, "", [new("x", MarkdownStyle.None, "file:///private", null)]))));
checks.Add("Every truncated response and malformed schema/tag/style/UTF-8/link rejected.");

foreach (var link in new[] { "https://example.test/", "HTTPS://example.test/a_(b)", "mailto:user@example.test" })
    Check(MarkdownPresentation.IsSafeLink(link), "approved link");
foreach (var link in new[] { "https://", "https://user:pass@example.test/", "javascript:alert(1)", "file:///private", "https://example.test/\n", "https://example.test/\\key" })
    Check(!MarkdownPresentation.IsSafeLink(link), "inert unsafe link");

if (args.Length > 1)
{
    foreach (var file in Directory.EnumerateFiles(Path.GetFullPath(args[1]), "*.bin"))
    {
        var document = MarkdownPresentation.Decode(File.ReadAllBytes(file));
        var json = System.Text.Json.JsonSerializer.Serialize(document);
        File.WriteAllText(Path.Combine(output, Path.GetFileNameWithoutExtension(file) + ".json"), json);
    }
    checks.Add("Real Rust wire output decoded.");
}

foreach (var theme in new[] { ThemeVariant.Light, ThemeVariant.Dark })
foreach (var scale in new[] { 1d, 1.25d, 1.5d, 2d })
foreach (var width in new[] { 360d, 640d })
{
    Application.Current.RequestedThemeVariant = theme;
    // The small harness loads the application's actual palette tokens, without
    // starting its wallet/network services or copying a second theme system.
    var repository = Path.GetFullPath(Path.Combine(AppContext.BaseDirectory, "../../../../../../"));
    var palette = XDocument.Load(Path.Combine(repository, $"MagicalCryptoWallet.Fluent/Styles/Themes/{theme}.axaml"));
    XNamespace x = "http://schemas.microsoft.com/winfx/2006/xaml";
    foreach (var element in palette.Root!.Elements().Where(e => e.Name.LocalName == "Color"))
        Application.Current.Resources[element.Attribute(x + "Key")!.Value] = Color.Parse(element.Value.Trim());
    Application.Current.Resources["TextForegroundColor"] = new SolidColorBrush((Color)Application.Current.Resources["SystemBaseHighColor"]!);
    var control = new ReleaseHighlightsText { Markdown = "fixture", Margin = new Thickness(24) };
    var window = new Window { Width = width, Height = 640, Content = control, Title = "Release highlights verification" };
    var platform = window.PlatformImpl ?? throw new InvalidOperationException("No headless platform.");
    var field = platform.GetType().GetField("<RenderScaling>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)
        ?? throw new InvalidOperationException("Headless scaling unavailable.");
    field.SetValue(platform, scale);
    var changed = platform.GetType().GetProperty("ScalingChanged")?.GetValue(platform) as Action<double>;
    changed?.Invoke(scale);
    window.Show(); Pump(() => !control.IsLoading);
    Check(!control.HasError, "render completed");
    var body = control.GetVisualDescendants().OfType<TextBlock>().First(t => t.Classes.Contains("releaseBody"));
    Check(body.Foreground is ISolidColorBrush foreground && foreground.Color == (Color)Application.Current.Resources["SystemBaseHighColor"]!, "real palette foreground applied");
    var links = control.GetVisualDescendants().OfType<HyperlinkButton>().ToArray();
    Check(links.Length == 1, "formatted adjacent link spans group into one keyboard target");
    var activated = "";
    control.LinkClicked += (_, target) => activated = target;
    Check(links[0].Focus(), "link accepts keyboard focus");
    window.KeyPressQwerty(PhysicalKey.Enter, RawInputModifiers.None);
    window.KeyReleaseQwerty(PhysicalKey.Enter, RawInputModifiers.None);
    Dispatcher.UIThread.RunJobs();
    Check(activated == "https://example.test/release", "Enter activates approved link");
    window.Measure(new Size(width, 640)); window.Arrange(new Rect(0, 0, width, 640));
    Dispatcher.UIThread.RunJobs();
    using var bitmap = window.CaptureRenderedFrame() ?? throw new InvalidOperationException("No rendered frame.");
    Check(bitmap.PixelSize == PixelSize.FromSize(new Size(width, 640), scale), "native scaling");
    bitmap.Save(Path.Combine(output, $"release-{theme}-{width}-{scale.ToString(System.Globalization.CultureInfo.InvariantCulture)}.png"));
    window.Close();
    Check(!control.IsLoading && !control.GetVisualDescendants().OfType<HyperlinkButton>().Any(), "detach clears presentation");
}
checks.Add("16 light/dark, narrow/wide and 100/125/150/200 percent captures; keyboard link activation.");

service.Hold();
var delayed = new ReleaseHighlightsText { Markdown = "old" };
var delayedWindow = new Window { Width = 360, Height = 400, Content = delayed };
delayedWindow.Show();
Check(delayed.IsLoading, "request is asynchronous");
var old = service.Pending;
var oldLoad = delayed.LoadCompletion;
delayed.Markdown = "new";
var latest = service.Pending;
Check(old.Token.IsCancellationRequested, "replacement cancels previous request");
latest.Completion.SetResult(specimen); Pump(() => !delayed.IsLoading);
old.Completion.SetException(new IOException("synthetic old response")); Pump(() => oldLoad.IsCompleted);
Check(!delayed.HasError, "stale error cannot replace newer content");
delayed.Markdown = "closed";
var closed = service.Pending;
var closedLoad = delayed.LoadCompletion;
delayedWindow.Close();
Check(closed.Token.IsCancellationRequested, "detach cancels request");
closed.Completion.SetResult(specimen); Pump(() => closedLoad.IsCompleted);
Check(!delayed.IsLoading, "late response does not resurrect detached content");
var failing = new ReleaseHighlightsText { Markdown = "failing" };
var errorWindow = new Window { Width = 360, Height = 400, Content = failing };
errorWindow.Show();
service.Pending.Completion.SetException(new IOException("synthetic bridge failure")); Pump(() => !failing.IsLoading);
Check(failing.HasError, "explicit unavailable presentation");
errorWindow.Close();
checks.Add("Async loading, replacement/detach cancellation, stale responses, and bridge error behavior.");
File.WriteAllLines(Path.Combine(output, "checks.txt"), checks);
Console.WriteLine(string.Join(Environment.NewLine, checks));

void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
void ExpectInvalid(Action action)
{
    try { action(); } catch (IOException) { return; }
    throw new InvalidOperationException("Malformed presentation accepted.");
}
void Pump(Func<bool> done)
{
    var deadline = DateTime.UtcNow.AddSeconds(10);
    while (!done()) { Dispatcher.UIThread.RunJobs(); if (DateTime.UtcNow > deadline) throw new TimeoutException("Presentation did not complete."); Thread.Sleep(5); }
    Dispatcher.UIThread.RunJobs();
}
byte[] Document(params MarkdownBlock[] blocks)
{
    using var bytes = new MemoryStream();
    using var writer = new BinaryWriter(bytes, new UTF8Encoding(false, true));
    writer.Write((byte)1); writer.Write((uint)blocks.Length);
    foreach (var block in blocks)
    {
        writer.Write((byte)block.Kind); writer.Write(block.Level); writer.Write(block.Depth); Text(block.Marker); writer.Write((uint)block.Runs.Count);
        foreach (var run in block.Runs) { writer.Write((byte)run.Style); Text(run.Text); Text(run.Link ?? ""); Text(run.Title ?? ""); }
    }
    return bytes.ToArray();
    void Text(string value) { var encoded = Encoding.UTF8.GetBytes(value); writer.Write((uint)encoded.Length); writer.Write(encoded); }
}

sealed class FixtureService : IMcwApplicationServices
{
    private byte[]? _response;
    public CancellationToken Stopped => CancellationToken.None;
    public (TaskCompletionSource<byte[]> Completion, CancellationToken Token) Pending { get; private set; }
    public void Complete(byte[] response) => _response = response;
    public void Hold() => _response = null;
    public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
    {
        if (operation != MarkdownPresentation.Operation || payload.Span[0] != MarkdownPresentation.Schema) throw new InvalidOperationException("Wrong operation/schema.");
        if (_response is { } response) return Task.FromResult(response);
        Pending = (new(TaskCreationOptions.RunContinuationsAsynchronously), cancellationToken);
        return Pending.Completion.Task;
    }
}
