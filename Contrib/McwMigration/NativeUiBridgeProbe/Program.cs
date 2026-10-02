using System.Text.Json;
using MagicalCryptoWallet.Announcements;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.NativeUi;

// Synthetic, dev-only child exercises shipping ManagedApplicationHost.Connect,
// the common service connection, real Rust host dispatch and the typed adapter.
if (Environment.GetEnvironmentVariable("MCW_HOSTED") != "1" || args.Length != 2) return 2;
using var host = ManagedApplicationHost.Connect();
var report = Path.GetFullPath(args[0]);
var inputs = Path.GetFullPath(args[1]);
var documents = 0;
var highlights = new ReleaseHighlights();
var current = await MarkdownPresentation.ParseAsync(highlights.MarkdownText);
var specimen = await MarkdownPresentation.ParseAsync("## Summary\n\n- **Native** path\n\n[Link](https://example.test/)");
if (specimen.Blocks.Count != 3 || specimen.Blocks[0].Kind != MarkdownBlockKind.Heading
    || specimen.Blocks[1].Runs[0].Style != MarkdownStyle.Bold || specimen.Blocks[2].Runs[0].Link != "https://example.test/")
    throw new InvalidOperationException("Real Markdown host presentation mismatch.");
foreach (var input in Directory.EnumerateFiles(inputs, "*.md"))
{
    var document = await MarkdownPresentation.ParseAsync(await File.ReadAllTextAsync(input));
    documents++;
    File.WriteAllText(Path.Combine(Path.GetDirectoryName(report)!, Path.GetFileNameWithoutExtension(input) + ".host.json"), JsonSerializer.Serialize(document));
}
await Rejected(new byte[] { 1, 0xff });
await Rejected(new byte[] { 2, (byte)'x' });
await Rejected(new byte[] { 1, 0 });
var oversized = new byte[MarkdownPresentation.MaximumInputBytes + 2]; oversized[0] = 1;
await Rejected(oversized);
// Requests and cancellation share the existing stream. It remains usable after
// a canceled wait; a bounded stateless parse may finish before CANCEL is read.
using var cancel = new CancellationTokenSource();
var pending = MarkdownPresentation.ParseAsync(new string('a', 100_000), cancel.Token);
cancel.Cancel();
var canceled = false;
try { await pending; } catch (OperationCanceledException) { canceled = true; }
var concurrent = await Task.WhenAll(Enumerable.Range(0, 12).Select(i => MarkdownPresentation.ParseAsync($"## Message {i}\n\n- item")));
if (concurrent.Any(d => d.Blocks.Count != 2)) throw new InvalidOperationException("Concurrent shared-stream requests failed.");
File.WriteAllText(report, JsonSerializer.Serialize(new { realHost = true, operation = "0x1100", documents, currentBlocks = current.Blocks.Count,
    malformedRejected = 4, canceled, concurrent = concurrent.Length, utc = DateTime.UtcNow }));
return 0;

static async Task Rejected(byte[] payload)
{
    try { await McwApplicationServices.Current.RequestAsync(MarkdownPresentation.Operation, payload); }
    catch (IOException) { return; }
    throw new InvalidOperationException("Invalid Markdown request accepted.");
}
