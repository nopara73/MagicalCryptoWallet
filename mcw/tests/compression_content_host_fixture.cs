// A synthetic managed child of the actual mcw host. Uses the retained fee caller,
// real cached factory/retry/transport, real SOCKS socket and real host adapter.
// The loopback SOCKS fixture never connects to, or resolves, its requested target.
using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.IO;
using System.IO.Compression;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.FeeRateEstimation;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Content;
using MagicalCryptoWallet.WebClients.MagicalCryptoWallet;

internal static class ContentHostFixture
{
	private const string Onion = "mempoolhqx4isw62xs7abwphsq7ldayuidyx2v2oethdhhj6mlo2r6ad.onion";
	private static readonly byte[] Plain = Encoding.UTF8.GetBytes("{\"fastestFee\":8.25,\"halfHourFee\":6,\"hourFee\":4,\"economyFee\":2}");
	private static void Check(bool condition, string name)
	{ if (!condition) { throw new InvalidOperationException(name); } }
	public static async Task<int> Main(string[] arguments)
	{
		if (Environment.GetEnvironmentVariable("MCW_HOSTED") != "1") { throw new InvalidOperationException("Actual mcw host required."); }
		if (arguments.Length != 0 && arguments[0].StartsWith("raw-", StringComparison.Ordinal))
		{ return await ContentRawHost.Run(arguments).ConfigureAwait(false); }
		using var host = ManagedApplicationHost.Connect();
		Logger.Configure(filePath: Path.Combine(AppContext.BaseDirectory, "synthetic-content-fixture.log"), logModes: [LogMode.Console]);
		var count = 0;
		foreach (var coding in new[] { "identity", "gzip", "deflate", "br", "gzip, br" })
		{
			await Caller(coding, coding is "br" or "deflate", false, false).ConfigureAwait(false);
			count++; Console.WriteLine("HOST PASS retained-fee-caller " + coding);
		}
		await Caller("gzip", false, true, false).ConfigureAwait(false);count++;Console.WriteLine("HOST PASS corrupt-checksum-withheld");
		await Caller("unknown", false, false, true).ConfigureAwait(false);count++;Console.WriteLine("HOST PASS unsupported-coding-withheld");
		await ResponseMetadata().ConfigureAwait(false);count++;Console.WriteLine("HOST PASS actual-decoded-response-metadata");
		await BodyAcquisitionCancellation().ConfigureAwait(false);count++;Console.WriteLine("HOST PASS retained-caller-body-acquisition-cancellation");
		await ReverseLayers(host).ConfigureAwait(false); count++; Console.WriteLine("HOST PASS four-reverse-coding-proofs");
		count += await NativeBounds(host).ConfigureAwait(false);
		Console.WriteLine($"ACTUAL CONTENT HOST RESULT: {count} passed; actual_application_host=true; synthetic_only=true");
		return 0;
	}
	private static async Task Caller(string coding, bool chunked, bool corrupt, bool unsupported)
	{
		var body = coding switch { "gzip, br" => Packed(Packed(Plain, "gzip"), "br"), "unknown" => Plain, _ => Packed(Plain, coding) };
		if (corrupt) { body[^8] ^= 1; }
		using var cancel = new CancellationTokenSource(TimeSpan.FromSeconds(20));
		using var server = new SocksFixture(body, coding, chunked, cancel.Token);
		var factory = new SpyFactory(server.Proxy);
		try
		{
			using var unrelated = factory.CreateClient("content-fixture-unrelated");
			Check(factory.Handlers.Single(v => v.Name == "content-fixture-unrelated").Handler.AutomaticDecompression == DecompressionMethods.All, "unrelated managed decoder unchanged");
			var provider = FeeRateProviders.MempoolSpaceAsync(factory);
			try
			{
				var rates = await provider(cancel.Token).ConfigureAwait(false);
				Check(!corrupt && !unsupported, "bad content reached fee parser");
				Check(rates.GetFeeRate(2).SatoshiPerByte == 8.25m && rates.GetFeeRate(3).SatoshiPerByte == 6m
					&& rates.GetFeeRate(6).SatoshiPerByte == 4m && rates.GetFeeRate(72).SatoshiPerByte == 2m, "exact retained decimal rates");
			}
			catch (McwContentDecodingException e)
			{
				Check(corrupt && e.Failure == McwContentFailure.Checksum || unsupported && e.Failure == McwContentFailure.UnsupportedEncoding, "actual host safe error classification");
			}
			await server.Complete.ConfigureAwait(false);
			Check(factory.Handlers.Single(v => v.Name == McwContentDecodingHandler.ClientName).Handler.AutomaticDecompression == DecompressionMethods.None, "selected transport decoder disabled");
		}
		finally { factory.Close(); }
	}
	private static async Task ResponseMetadata()
	{
		var bytes = Packed(Plain, "gzip");using var cancel = new CancellationTokenSource(TimeSpan.FromSeconds(20));
		using var server = new SocksFixture(bytes, "gzip", false, cancel.Token);var factory = new SpyFactory(server.Proxy);
		try
		{
			using var client = factory.CreateClient(McwContentDecodingHandler.ClientName);
			using var response = await client.GetAsync("http://" + Onion + "/api/v1/fees/precise", cancel.Token).ConfigureAwait(false);
			var content = response.Content as McwDecodedHttpContent ?? throw new InvalidOperationException("Rust decoder adapter absent");
			Check(content.EncodedLength == bytes.Length && content.EncodedContentLength == bytes.Length && content.DecodedLength == Plain.Length, "wire length proof retained");
			Check(content.Headers.ContentLength == Plain.Length && content.Headers.ContentEncoding.Count == 0 && content.Headers.ContentType?.CharSet == "utf-8", "decoded representation headers");
			Check(content.Layers.Count == 1 && content.Layers[0].Consumed == bytes.Length && content.Layers[0].Coding == McwContentCoding.Gzip, "actual native layer proof");
			Check((await content.ReadAsByteArrayAsync(cancel.Token).ConfigureAwait(false)).SequenceEqual(Plain), "actual native decoded bytes");
			await server.Complete.ConfigureAwait(false);
		}
		finally { factory.Close(); }
	}
	private static async Task BodyAcquisitionCancellation()
	{
		using var overall = new CancellationTokenSource(TimeSpan.FromSeconds(20));
		using var server = new SocksFixture(Packed(Plain, "gzip"), "gzip", false, overall.Token, stall: true);
		var factory = new SpyFactory(server.Proxy);using var caller = new CancellationTokenSource();
		try
		{
			var pending = FeeRateProviders.MempoolSpaceAsync(factory)(caller.Token);
			await server.HeadersSent.Task.WaitAsync(overall.Token).ConfigureAwait(false);caller.Cancel();
			try { await pending.ConfigureAwait(false); throw new InvalidOperationException("retained caller ignored cancellation"); }
			catch (OperationCanceledException) { }
			overall.Cancel();
			try { await server.Complete.ConfigureAwait(false); } catch (OperationCanceledException) { }
		}
		finally { factory.Close(); }
	}
	private static byte[] Packed(byte[] input, string coding)
	{
		if (coding == "identity") { return input; }using var output = new MemoryStream();
		using (Stream encoder = coding switch { "gzip" => new GZipStream(output, CompressionLevel.Optimal, true), "deflate" => new ZLibStream(output, CompressionLevel.Optimal, true), "br" => new BrotliStream(output, CompressionLevel.Optimal, true), _ => throw new ArgumentException("oracle coding") })
		{ encoder.Write(input); }return output.ToArray();
	}
	private static async Task ReverseLayers(ManagedApplicationHost host)
	{
		var encoded = Packed(Packed(Packed(Packed(Plain, "gzip"), "deflate"), "br"), "gzip");
		var result = await McwContentDecoder.DecodeAsync(encoded, ["gzip", "deflate, br", "gzip"], host).ConfigureAwait(false);
		Check(result.Bytes.SequenceEqual(Plain), "four reverse layers exact bytes");
		Check(result.Layers.Select(v => v.Coding).SequenceEqual(new[] { McwContentCoding.Gzip, McwContentCoding.Brotli, McwContentCoding.Deflate, McwContentCoding.Gzip }), "four reverse layer order");
		Check(result.Layers.All(v => v.InputLength == v.Consumed), "four reverse layers exact consumption");
	}
	internal static byte[] NativePacket(byte[] body, params string[] fields)
	{
		using var stream = new MemoryStream(); using var writer = new BinaryWriter(stream, Encoding.ASCII, true);
		writer.Write((ushort)1); writer.Write((ushort)5000); writer.Write((byte)fields.Length); writer.Write((byte)0); writer.Write(body.Length);
		foreach (var field in fields) { var bytes = Encoding.ASCII.GetBytes(field); writer.Write((ushort)bytes.Length); writer.Write(bytes); }
		writer.Write(body); return stream.ToArray();
	}
	private static async Task<int> NativeBounds(ManagedApplicationHost host)
	{
		var good = NativePacket(Plain);
		var cases = new List<(string Name, byte[] Packet, ushort Failure)> {
			("empty-packet", [], 1), ("short-packet", new byte[9], 1),
			("truncated-body", good[..^1], 1), ("trailing-body", good.Concat(new byte[1]).ToArray(), 1),
			("encoded-body-bound", NativePacket(new byte[McwContentDecoder.MaxBody + 1]), 1),
			("field-bound", NativePacket([], new string('x', McwContentDecoder.MaxField + 1)), 1),
			("field-count-bound", NativePacket([], "identity", "identity", "identity", "identity", "identity"), 1),
			("coding-count-bound", NativePacket([], "identity,identity,identity,identity,identity"), 4),
			("invalid-encoding", NativePacket(Plain, "gzip\n"), 2),
			("unsupported-encoding", NativePacket(Plain, "unknown"), 3),
			("decoded-body-bound", NativePacket(Packed(Enumerable.Range(0, McwContentDecoder.MaxBody + 1).Select(i => (byte)i).ToArray(), "gzip"), "gzip"), 8),
		};
		foreach (var pair in new[] { (Index: 0, Value: (byte)2, Name: "packet-version"), (Index: 5, Value: (byte)1, Name: "reserved-byte") })
		{ var changed = good.ToArray(); changed[pair.Index] = pair.Value; cases.Add((pair.Name, changed, 1)); }
		foreach (ushort timeout in new ushort[] { 0, 5001 })
		{ var changed = good.ToArray(); BinaryPrimitives.WriteUInt16LittleEndian(changed.AsSpan(2), timeout); cases.Add(("deadline-bound-" + timeout, changed, 1)); }
		using var limit = new CancellationTokenSource(TimeSpan.FromSeconds(30));
		foreach (var item in cases)
		{
			var reply = await ((IMcwApplicationServices)host).RequestAsync(McwContentDecoder.Operation, item.Packet, limit.Token).ConfigureAwait(false);
			Check(reply.Length == 22 && reply[0] == 1 && reply[1] == 0 && reply[2] == 1, "private bounded native failure framing");
			Check(BinaryPrimitives.ReadUInt16LittleEndian(reply.AsSpan(3)) == item.Failure, "native classification " + item.Name);
			Array.Clear(reply); Console.WriteLine("HOST PASS native-boundary " + item.Name);
		}
		var after = await McwContentDecoder.DecodeAsync(Plain, [], host, limit.Token).ConfigureAwait(false);
		Check(after.Bytes.SequenceEqual(Plain), "connection usable after malformed packets");
		Console.WriteLine("HOST PASS native-boundary-sibling-after-errors"); return cases.Count + 1;
	}
	private sealed class SpyFactory(Uri proxy) : OnionHttpClientFactory(proxy)
	{
		public List<(string Name, HttpClientHandler Handler)> Handlers { get; } = [];
		protected override HttpClientHandler CreateHttpClientHandler(string name)
		{ var handler = base.CreateHttpClientHandler(name);Handlers.Add((name, handler));return handler; }
		public void Close() { foreach (var pair in Handlers) { pair.Handler.Dispose(); } }
	}
	private sealed class SocksFixture : IDisposable
	{
		private readonly TcpListener _listener;
		public Uri Proxy { get; }
		public Task Complete { get; }
		public TaskCompletionSource<bool> HeadersSent { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
		public SocksFixture(byte[] body, string coding, bool chunked, CancellationToken token, bool stall = false)
		{
			_listener = new(IPAddress.Loopback, 0);_listener.Start();
			Proxy = new("socks5://127.0.0.1:" + ((IPEndPoint)_listener.LocalEndpoint).Port);
			Complete = Serve(body, coding, chunked, token, stall);
		}
		private async Task Serve(byte[] body, string coding, bool chunked, CancellationToken token, bool stall)
		{
			using var socket = await _listener.AcceptTcpClientAsync(token).ConfigureAwait(false);using var stream = socket.GetStream();
			static async Task<byte[]> Read(Stream input, int n, CancellationToken ct)
			{ var data = new byte[n];await input.ReadExactlyAsync(data, ct).ConfigureAwait(false);return data; }
			var greeting = await Read(stream, 2, token).ConfigureAwait(false);Check(greeting[0] == 5, "SOCKS greeting");
			var methods = await Read(stream, greeting[1], token).ConfigureAwait(false);Check(methods.Contains((byte)2), "SOCKS credentials retained");
			await stream.WriteAsync(new byte[] { 5, 2 }, token).ConfigureAwait(false);
			var auth = await Read(stream, 2, token).ConfigureAwait(false);Check(auth[0] == 1, "SOCKS auth version");
			var user = Encoding.UTF8.GetString(await Read(stream, auth[1], token).ConfigureAwait(false));var passwordSize = (await Read(stream, 1, token).ConfigureAwait(false))[0];
			var password = Encoding.UTF8.GetString(await Read(stream, passwordSize, token).ConfigureAwait(false));
			Check(user == McwContentDecodingHandler.ClientName && password == user, "retained stream identity");
			await stream.WriteAsync(new byte[] { 1, 0 }, token).ConfigureAwait(false);
			var command = await Read(stream, 4, token).ConfigureAwait(false);Check(command.SequenceEqual(new byte[] { 5, 1, 0, 3 }), "remote name sent through SOCKS");
			var size = (await Read(stream, 1, token).ConfigureAwait(false))[0];var destination = Encoding.ASCII.GetString(await Read(stream, size, token).ConfigureAwait(false));
			var port = await Read(stream, 2, token).ConfigureAwait(false);Check(destination == Onion && port.SequenceEqual(new byte[] { 0, 80 }), "retained onion route");
			await stream.WriteAsync(new byte[] { 5, 0, 0, 1, 127, 0, 0, 1, 0, 0 }, token).ConfigureAwait(false);
			var request = new List<byte>();var octet = new byte[1];
			while (request.Count < 16384)
			{
				await stream.ReadExactlyAsync(octet, token).ConfigureAwait(false);request.Add(octet[0]);
				if (request.Count >= 4 && request.TakeLast(4).SequenceEqual(new byte[] { 13, 10, 13, 10 })) { break; }
			}
			var head = Encoding.ASCII.GetString(request.ToArray());Check(head.StartsWith("GET /api/v1/fees/precise HTTP/1.1\r\n", StringComparison.Ordinal), "retained HTTP request");
			Check(head.Contains("Accept-Encoding: gzip, deflate, br\r\n", StringComparison.OrdinalIgnoreCase), "first-party advertised codings");
			var response = "HTTP/1.1 200 OK\r\nContent-Type: application/json; charset=utf-8\r\nConnection: close\r\nX-Synthetic: content-fixture\r\n";
			if (coding != "identity") { response += "Content-Encoding: " + coding + "\r\n"; }
			response += chunked ? "Transfer-Encoding: chunked\r\n\r\n" : "Content-Length: " + body.Length + "\r\n\r\n";
			await stream.WriteAsync(Encoding.ASCII.GetBytes(response), token).ConfigureAwait(false);HeadersSent.TrySetResult(true);
			if (stall) { await Task.Delay(Timeout.InfiniteTimeSpan, token).ConfigureAwait(false);return; }
			if (chunked)
			{
				foreach (var part in body.Chunk(7))
				{ await stream.WriteAsync(Encoding.ASCII.GetBytes(part.Length.ToString("x", System.Globalization.CultureInfo.InvariantCulture) + "\r\n"), token).ConfigureAwait(false);await stream.WriteAsync(part, token).ConfigureAwait(false);await stream.WriteAsync(new byte[] { 13, 10 }, token).ConfigureAwait(false); }
				await stream.WriteAsync(Encoding.ASCII.GetBytes("0\r\n\r\n"), token).ConfigureAwait(false);
			}
			else { await stream.WriteAsync(body, token).ConfigureAwait(false); }
		}
		public void Dispose() { _listener.Stop(); }
	}
}
