using System;
using System.Buffers;
using System.Collections.Generic;
using System.IO;
using System.IO.Pipelines;
using System.Linq;
using System.Net;
using System.Net.Sockets;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Tor.Control;
using MagicalCryptoWallet.Tor.Control.Exceptions;
using MagicalCryptoWallet.Tor.Control.Messages;

// Non-shipping synthetic managed child. Uses the real host/bridge and production
// reader, never a managed reference parser or a fake successful service response.
if (args.Length != 2) { return 2; }
var report = Path.GetFullPath(args[0]);
var fixtures = Path.GetFullPath(args[1]);
using var host = ManagedApplicationHost.Connect();
using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(120));
host.BindShutdown(timeout.Cancel);
var token = timeout.Token;
var count = 0;

var vectors = File.ReadLines(fixtures).Where(s => !s.StartsWith('#')).Select(s => s.Split('\t')).ToArray();
foreach (var vector in vectors)
{
	var wire = Convert.FromHexString(vector[1]);
	var status = int.Parse(vector[2], System.Globalization.CultureInfo.InvariantCulture);
	var lines = vector[3].Split(',').Select(s => Encoding.ASCII.GetString(Convert.FromHexString(s))).ToArray();
	var pipe = NewPipe();
	await pipe.Writer.WriteAsync(wire, token);
	await pipe.Writer.CompleteAsync();
	var reply = await TorControlReplyReader.ReadReplyAsync(pipe.Reader, token);
	Equal(reply, status, lines);
	await pipe.Reader.CompleteAsync();
	count++;

	// The reader exposes one additional byte on every ReadAsync. This forces
	// every fragmentation boundary through the actual Rust bridge operation.
	var fragmented = new FragmentedReader(wire, fragmentSize: 1);
	Equal(await TorControlReplyReader.ReadReplyAsync(fragmented, token), status, lines);
	if (fragmented.Consumed != wire.Length || fragmented.Reads != wire.Length) { throw new Exception("Fragmentation/consumption mismatch."); }
	count++;
}

// Several distinct replies in one pipe remain separately available.
var coalesced = NewPipe();
foreach (var vector in vectors) { await coalesced.Writer.WriteAsync(Convert.FromHexString(vector[1]), token); }
await coalesced.Writer.CompleteAsync();
foreach (var vector in vectors)
{
	Equal(await TorControlReplyReader.ReadReplyAsync(coalesced.Reader, token),
		int.Parse(vector[2], System.Globalization.CultureInfo.InvariantCulture),
		vector[3].Split(',').Select(s => Encoding.ASCII.GetString(Convert.FromHexString(s))).ToArray());
	count++;
}
await coalesced.Reader.CompleteAsync();

// A producer using the normal 64 KiB pipe backpressure must not stall while a
// multiline reply is still incomplete. Partial input belongs to the adapter's
// bounded raw-byte staging buffer, rather than the producer's pipe buffer.
var pressure = new Pipe();
var body = Encoding.ASCII.GetBytes("250+data\r\n" + string.Concat(Enumerable.Repeat(new string('a', 1024) + "\r\n", 100)) + ".\r\n250 OK\r\n");
var producing = Task.Run(async () =>
{
	for (var offset = 0; offset < body.Length; offset += 4096) { await pressure.Writer.WriteAsync(body.AsMemory(offset, Math.Min(4096, body.Length - offset)), token); }
	await pressure.Writer.CompleteAsync();
}, token);
var pressureReply = await TorControlReplyReader.ReadReplyAsync(pressure.Reader, token).WaitAsync(TimeSpan.FromSeconds(10), token);
if (pressureReply.ResponseLines.Count != 103 || pressureReply.ResponseLines[101] != "." || pressureReply.ResponseLines[102] != "250 OK") { throw new Exception("Backpressure reply mismatch."); }
await producing;
await pressure.Reader.CompleteAsync();
count++;

// Force a segmented sequence as well as fragmented reads, including CR at the
// end of one segment and LF at the start of the next and an earlier bare CR.
var lineReader = new FragmentedReader("bare\rbody\r\r\nnext\r\n"u8.ToArray(), 100, segmented: true);
if (await lineReader.ReadLineAsync(token) != "bare\rbody\r" || await lineReader.ReadLineAsync(token) != "next") { throw new Exception("CRLF segmentation mismatch."); }
count++;

await Error<TorControlReplyParseException>([], "No reply line was received.");
await Error<TorControlReplyParseException>("250 OK\r"u8.ToArray(), "No reply line was received.");
await Error<TorControlReplyParseException>("OK\r\n"u8.ToArray(), "Status code requires at least 3 characters.");
await Error<TorControlReplyParseException>("xx OK\r\n"u8.ToArray(), "Unknown status code: 'xx '.");
await Error<InvalidDataException>("250-partial\r\n"u8.ToArray(), "No more data.");
await Error<InvalidDataException>("250+data\r\n.\r\n250 OK\r"u8.ToArray(), "Incomplete message.");
var oversized = Enumerable.Repeat((byte)'a', 65539).ToArray();
await Error<TorControlReplyParseException>(oversized, "Tor control parsing limit exceeded.");
var eofLine = NewPipe();
await eofLine.Writer.CompleteAsync();
try { await eofLine.Reader.ReadLineAsync(token); throw new Exception("Line EOF accepted."); }
catch (InvalidDataException e) when (e.Message == "No more data.") { count++; }
await eofLine.Reader.CompleteAsync();

// Cancellation of a partial reply must release the current PipeReader read.
var canceled = NewPipe();
using (var cancel = new CancellationTokenSource())
{
	await canceled.Writer.WriteAsync("250+partial\r\n"u8.ToArray(), token);
	var pending = TorControlReplyReader.ReadReplyAsync(canceled.Reader, cancel.Token);
	await Task.Delay(25, token);
	cancel.Cancel();
	try { await pending.WaitAsync(TimeSpan.FromSeconds(3), token); throw new Exception("Cancellation accepted."); }
	catch (OperationCanceledException) { count++; }
}
await canceled.Reader.CompleteAsync();
await canceled.Writer.CompleteAsync();

// A CancelPendingRead result is a cancellation even when its token remains live.
var cancelRead = NewPipe();
var reading = TorControlReplyReader.ReadReplyAsync(cancelRead.Reader, token);
cancelRead.Reader.CancelPendingRead();
try { await reading.WaitAsync(TimeSpan.FromSeconds(3), token); throw new Exception("Pipe cancellation accepted."); }
catch (OperationCanceledException) { count++; }
await cancelRead.Reader.CompleteAsync();
await cancelRead.Writer.CompleteAsync();

// Existing TorControlClient routes a real Rust-parsed 650 notification and 250
// synchronous response on a synthetic local TCP connection. No Tor/wallet keys.
using (var listener = new TcpListener(IPAddress.Loopback, 0))
{
	listener.Start();
	using var connection = new TcpClient();
	var accept = listener.AcceptTcpClientAsync(token);
	await connection.ConnectAsync((IPEndPoint)listener.LocalEndpoint, token);
	using var server = await accept;
	await using var control = new TorControlClient(connection);
	await using var events = control.ReadEventsAsync(token).GetAsyncEnumerator(token);
	var nextEvent = events.MoveNextAsync().AsTask();
	var command = control.GetConfAsync("SocksPort", token);
	using var serverReader = new StreamReader(server.GetStream(), Encoding.ASCII, leaveOpen: true);
	if (await serverReader.ReadLineAsync(token) != "GETCONF SocksPort") { throw new Exception("Tor command changed."); }
	await server.GetStream().WriteAsync("650 CIRC 1 BUILT synthetic\r\n250 SOCKSPORT=38150\r\n"u8.ToArray(), token);
	Equal(await command, 250, ["SOCKSPORT=38150"]);
	if (!await nextEvent || events.Current.StatusCode != StatusCode.AsynchronousEventNotify) { throw new Exception("Tor event routing changed."); }
	server.Close();
	count++;
}

// Rust errors stay typed; unknown operations and malformed EOF flags do not
// damage the shared bridge or cause local grammar fallback.
try { await McwApplicationServices.Current.RequestAsync(0x0f00, new byte[] {2}, token); throw new Exception("Invalid Rust request accepted."); }
catch (IOException) { count++; }
var sanity = NewPipe();
await sanity.Writer.WriteAsync("250 OK\r\n"u8.ToArray(), token);
await sanity.Writer.CompleteAsync();
Equal(await TorControlReplyReader.ReadReplyAsync(sanity.Reader, token), 250, ["OK"]);
await sanity.Reader.CompleteAsync();
count++;

// Stopping the actual bridge also cancels a reader waiting for Tor input.
var stopping = NewPipe();
var waiting = TorControlReplyReader.ReadReplyAsync(stopping.Reader, token);
host.Dispose();
try { await waiting.WaitAsync(TimeSpan.FromSeconds(3), token); throw new Exception("Host stop did not cancel the reader."); }
catch (OperationCanceledException) { count++; }
await stopping.Reader.CompleteAsync();
await stopping.Writer.CompleteAsync();

var unbound = NewPipe();
await unbound.Writer.WriteAsync("250 OK\r\n"u8.ToArray());
await unbound.Writer.CompleteAsync();
try { await TorControlReplyReader.ReadReplyAsync(unbound.Reader, CancellationToken.None); throw new Exception("Unhosted wallet parsed locally."); }
catch (InvalidOperationException) { count++; }
await unbound.Reader.CompleteAsync();

// Transport fault injection only: no fake successful parser response exists.
// Known-valid input must still fail if its Rust response packet is malformed.
foreach (var bad in new byte[][] { [], [0,0], [3], [1], [2,6], [2,2,2], [2,4,0],
	[1,0,0,0,0,250,0,0,0,1,0,0,0,2,0,0,0,79,75],
	[1,8,0,0,0,250,0,0,0,0,0,0,0],
	[1,8,0,0,0,250,0,0,0,1,0,0,0,255,255,255,255],
	[1,8,0,0,0,250,0,0,0,1,0,0,0,2,0,0,0,128,75],
	[1,8,0,0,0,250,0,0,0,1,0,0,0,2,0,0,0,79,75,0], [0] })
{
	using var binding = McwApplicationServices.Bind(new FaultResponse(bad));
	var pipe = NewPipe();
	await pipe.Writer.WriteAsync("250 OK\r\n"u8.ToArray());
	await pipe.Writer.CompleteAsync();
	try { await TorControlReplyReader.ReadReplyAsync(pipe.Reader, CancellationToken.None); throw new Exception("Malformed Rust packet accepted."); }
	catch (IOException e) when (e.Message == "Invalid mcw Tor control codec response.") { count++; }
	await pipe.Reader.CompleteAsync();
}

File.WriteAllText(report, JsonSerializer.Serialize(new { assertions = count, oracleReplies = vectors.Length, productionRustBridge = true, coordinatorExcluded = true }));
return 0;

static Pipe NewPipe() => new(new PipeOptions(pauseWriterThreshold: 0));
static void Equal(TorControlReply reply, int status, string[] lines)
{
	if ((int)reply.StatusCode != status || !reply.ResponseLines.SequenceEqual(lines, StringComparer.Ordinal)) { throw new Exception("Tor reply compatibility mismatch."); }
}
async Task Error<T>(byte[] bytes, string expected) where T : Exception
{
	var pipe = NewPipe();
	await pipe.Writer.WriteAsync(bytes, token);
	await pipe.Writer.CompleteAsync();
	try { await TorControlReplyReader.ReadReplyAsync(pipe.Reader, token); throw new Exception("Invalid Tor reply accepted."); }
	catch (T e) when (e.Message == expected) { count++; }
	finally { await pipe.Reader.CompleteAsync(); }
}

sealed class FaultResponse(byte[] response) : IMcwApplicationServices
{
	public CancellationToken Stopped => CancellationToken.None;
	public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default) => Task.FromResult(response);
}

sealed class FragmentedReader(byte[] wire, int fragmentSize, bool segmented = false) : PipeReader
{
	private int _visible;
	private bool _reading;
	private bool _canceled;
	private ReadOnlySequence<byte> _buffer;
	public int Consumed { get; private set; }
	public int Reads { get; private set; }
	public override ValueTask<ReadResult> ReadAsync(CancellationToken cancellationToken = default)
	{
		cancellationToken.ThrowIfCancellationRequested();
		if (_reading) { throw new InvalidOperationException("Prior read was not advanced."); }
		_visible = Math.Min(wire.Length, _visible + fragmentSize);
		var memory = wire.AsMemory(Consumed, _visible - Consumed);
		_buffer = segmented && memory.Length > 1 ? Segments(memory) : new ReadOnlySequence<byte>(memory);
		_reading = true;
		Reads++;
		return ValueTask.FromResult(new ReadResult(_buffer, _canceled, _visible == wire.Length));
	}
	public override void AdvanceTo(SequencePosition consumed) => AdvanceTo(consumed, consumed);
	public override void AdvanceTo(SequencePosition consumed, SequencePosition examined)
	{
		if (!_reading) { throw new InvalidOperationException("Advance without read."); }
		Consumed += (int)_buffer.Slice(0, consumed).Length;
		_reading = false;
	}
	public override bool TryRead(out ReadResult result) { result = default; return false; }
	public override void CancelPendingRead() => _canceled = true;
	public override void Complete(Exception? exception = null) { }
	private static ReadOnlySequence<byte> Segments(ReadOnlyMemory<byte> memory)
	{
		var first = new Segment(memory[..1]);
		var last = first;
		for (var i = 1; i < memory.Length; i++) { last = last.Append(memory.Slice(i, 1)); }
		return new ReadOnlySequence<byte>(first, 0, last, last.Memory.Length);
	}
	private sealed class Segment : ReadOnlySequenceSegment<byte>
	{
		public Segment(ReadOnlyMemory<byte> memory) { Memory = memory; }
		public Segment Append(ReadOnlyMemory<byte> memory)
		{
			var next = new Segment(memory) { RunningIndex = RunningIndex + Memory.Length };
			Next = next;
			return next;
		}
	}
}
