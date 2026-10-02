// Component tests: real Rust payload decoder and actual managed leaf/interface.
// The line driver is verification tooling, not the application bridge/host.
using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Content;

internal static class ContentAdapterTests
{
	private static int _passed;
	private static void Require(bool condition, string name)
	{ if (!condition) { throw new InvalidOperationException(name); } }
	private static async Task Test(string name, Func<Task> action)
	{ await action().ConfigureAwait(false); _passed++; Console.WriteLine("PASS " + name); }

	public static async Task<int> Main(string[] args)
	{
		if (args.Length != 1) { throw new ArgumentException("Actual Rust test driver path required."); }
		using var services = new RustPayloadServices(args[0]);
		using var binding = McwApplicationServices.Bind(services);
		var plain = Encoding.UTF8.GetBytes("{\"fastestFee\":8.25,\"halfHourFee\":6,\"hourFee\":4,\"economyFee\":2}");
		foreach (var coding in new[] { "identity", "gzip", "deflate", "br", "gzip, br", "x-gzip" })
		{
			await Test("actual-rust-" + coding, async () =>
			{
				var encoded = coding switch { "gzip, br" => Compress(Compress(plain, "gzip"), "br"), _ => Compress(plain, coding) };
				var original = new TrackingContent(encoded); original.Headers.ContentType = new("application/json") { CharSet = "utf-8" };
				if (coding != "identity") { original.Headers.TryAddWithoutValidation("Content-Encoding", coding); }
				original.Headers.TryAddWithoutValidation("X-Representation", "synthetic");
				var before = services.Calls;
				using var client = new HttpClient(new McwContentDecodingHandler(new FixedResponse(original)));
				using var response = await client.GetAsync("http://synthetic.invalid/fee").ConfigureAwait(false);
				Require(original.Disposed, "original content disposal");
				Require(services.Calls == before + 1, "decode exactly once");
				Require((await response.Content.ReadAsByteArrayAsync().ConfigureAwait(false)).SequenceEqual(plain), "exact decoded bytes");
				Require(response.Content.Headers.ContentEncoding.Count == 0 && response.Content.Headers.ContentLength == plain.Length, "decoded headers");
				Require(response.Content.Headers.ContentType?.CharSet == "utf-8" && response.Content.Headers.Contains("X-Representation"), "preserved headers");
				var proof = (McwDecodedHttpContent)response.Content;
				Require(proof.EncodedLength == encoded.Length && proof.DecodedLength == plain.Length && proof.EncodedContentLength == encoded.Length, "wire length metadata");
				Require(proof.Layers.All(l => l.Consumed == l.InputLength), "full input proof");
			}).ConfigureAwait(false);
		}
		await Test("checksum-error-withheld-and-disposed", async () =>
		{
			var bytes = Compress(plain, "gzip"); bytes[^8] ^= 1; var original = new TrackingContent(bytes); original.Headers.ContentEncoding.Add("gzip");
			using var client = new HttpClient(new McwContentDecodingHandler(new FixedResponse(original)));
			try { using var response = await client.GetAsync("http://synthetic.invalid/").ConfigureAwait(false); throw new InvalidOperationException("corruption accepted"); }
			catch (McwContentDecodingException e) { Require(e.Failure == McwContentFailure.Checksum && e.OutputProduced == (ulong)plain.Length, "checksum classification"); }
			Require(original.Disposed, "error disposal");
		}).ConfigureAwait(false);
		await Test("unsupported-encoding-no-fallback", async () =>
		{
			var original = new TrackingContent(plain); original.Headers.ContentEncoding.Add("unknown");
			using var client = new HttpClient(new McwContentDecodingHandler(new FixedResponse(original)));
			try { using var response = await client.GetAsync("http://synthetic.invalid/").ConfigureAwait(false); throw new InvalidOperationException("unsupported accepted"); }
			catch (McwContentDecodingException e) { Require(e.Failure == McwContentFailure.UnsupportedEncoding, "unsupported classification"); }
			Require(original.Disposed, "unsupported disposal");
		}).ConfigureAwait(false);
		await Test("encoded-limit-before-service", async () =>
		{
			var original = new TrackingContent(new byte[McwContentDecoder.MaxBody + 1]); var before = services.Calls;
			using var client = new HttpClient(new McwContentDecodingHandler(new FixedResponse(original)));
			try { using var response = await client.GetAsync("http://synthetic.invalid/").ConfigureAwait(false); throw new InvalidOperationException("oversized accepted"); }
			catch (IOException) { }
			Require(original.Disposed && services.Calls == before, "bounded preflight");
		}).ConfigureAwait(false);
		await Test("expanded-limit-no-body-returned", async () =>
		{
			var original = new TrackingContent(Compress(new byte[McwContentDecoder.MaxBody + 1], "gzip")); original.Headers.ContentEncoding.Add("gzip");
			using var client = new HttpClient(new McwContentDecodingHandler(new FixedResponse(original)));
			try { using var response = await client.GetAsync("http://synthetic.invalid/").ConfigureAwait(false); throw new InvalidOperationException("expanded body accepted"); }
			catch (McwContentDecodingException e) { Require(e.Failure is McwContentFailure.OutputLimit or McwContentFailure.ExpansionLimit, "expanded limit"); }
			Require(original.Disposed, "expanded disposal");
		}).ConfigureAwait(false);
		await Test("truncated-member-no-body-returned", async () =>
		{
			var bytes = Compress(plain, "gzip");var original = new TrackingContent(bytes[..^1]); original.Headers.ContentEncoding.Add("gzip");
			using var client = new HttpClient(new McwContentDecodingHandler(new FixedResponse(original)));
			try { using var response = await client.GetAsync("http://synthetic.invalid/").ConfigureAwait(false); throw new InvalidOperationException("truncated accepted"); }
			catch (McwContentDecodingException e) { Require(e.Failure == McwContentFailure.Truncated, "truncated classification"); }
		}).ConfigureAwait(false);
		await Test("cancellation-propagates-before-service", async () =>
		{
			using var cancellation = new CancellationTokenSource(); cancellation.Cancel(); var before = services.Calls;
			try { await McwContentDecoder.DecodeAsync(plain, [], services, cancellation.Token).ConfigureAwait(false); throw new InvalidOperationException("cancellation ignored"); }
			catch (OperationCanceledException) { }
			Require(services.Calls == before, "cancelled input not submitted");
		}).ConfigureAwait(false);
		await Test("metadata-error-does-not-log-plaintext", async () =>
		{
			var result = await McwContentDecoder.DecodeAsync(plain, [], services).ConfigureAwait(false);
			Require(!result.ToString().Contains("fastestFee", StringComparison.Ordinal), "safe result text");
		}).ConfigureAwait(false);
		await Test("invalid-native-proof-fails-closed", async () =>
		{
			var p = new byte[12 + plain.Length];p[0] = 1;BinaryPrimitives.WriteInt32LittleEndian(p.AsSpan(3), plain.Length - 1);BinaryPrimitives.WriteInt32LittleEndian(p.AsSpan(7), plain.Length);plain.CopyTo(p,12);
			try { await McwContentDecoder.DecodeAsync(plain, [], new StaticReply(p)).ConfigureAwait(false); throw new InvalidOperationException("bad proof accepted"); }
			catch (IOException) { }
		}).ConfigureAwait(false);
		Console.WriteLine($"COMPONENT TEST RESULT: {_passed} passed; actual_application_host=false");
		return 0;
	}
	private static byte[] Compress(byte[] bytes, string coding)
	{
		if (coding == "identity") { return bytes; }
		using var output = new MemoryStream();
		using (Stream stream = coding switch { "gzip" or "x-gzip" => new GZipStream(output, CompressionLevel.Optimal, true), "deflate" => new ZLibStream(output, CompressionLevel.Optimal, true), "br" => new BrotliStream(output, CompressionLevel.Optimal, true), _ => throw new ArgumentException("oracle coding") })
		{ stream.Write(bytes); }
		return output.ToArray();
	}
	private sealed class TrackingContent(byte[] bytes) : ByteArrayContent(bytes)
	{
		public bool Disposed { get; private set; }
		protected override void Dispose(bool disposing) { Disposed = true; base.Dispose(disposing); }
	}
	private sealed class FixedResponse(HttpContent content) : HttpMessageHandler
	{
		protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
		{
			cancellationToken.ThrowIfCancellationRequested();
			Require(request.Headers.AcceptEncoding.Select(v => v.Value).SequenceEqual(new[] { "gzip", "deflate", "br" }), "negotiated codings");
			return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK) { Content = content });
		}
	}
	// Malformed protocol response test only; it performs no decoding.
	private sealed class StaticReply(byte[] reply) : IMcwApplicationServices
	{
		public CancellationToken Stopped => CancellationToken.None;
		public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default) => Task.FromResult(reply);
	}
	// Actual Rust payload algorithm, transported by a disposable verification driver.
	private sealed class RustPayloadServices(string driver) : IMcwApplicationServices, IDisposable
	{
		private readonly CancellationTokenSource _stop = new();
		public CancellationToken Stopped => _stop.Token;
		public int Calls { get; private set; }
		public async Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
		{
			Require(operation == McwContentDecoder.Operation, "operation"); Calls++;
			using var process = Process.Start(new ProcessStartInfo(driver) { UseShellExecute = false, CreateNoWindow = true, RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true }) ?? throw new IOException("Test driver unavailable.");
			await process.StandardInput.WriteLineAsync("P\t-\t" + Convert.ToHexString(payload.Span).ToLowerInvariant()).ConfigureAwait(false);
			process.StandardInput.Close();
			var line = await process.StandardOutput.ReadLineAsync(cancellationToken).ConfigureAwait(false) ?? throw new IOException("Test driver disconnected.");
			await process.WaitForExitAsync(cancellationToken).ConfigureAwait(false);
			Require(process.ExitCode == 0 && line.StartsWith("PACKET\t", StringComparison.Ordinal), "actual Rust driver response");
			return Convert.FromHexString(line[7..]);
		}
		public void Dispose() { _stop.Cancel(); _stop.Dispose(); }
	}
}
