using System;
using System.Buffers.Binary;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Threading.Channels;
using System.Threading.Tasks;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Crypto;

// Scripted wire faults exercise the actual managed transport's ownership rules.
// Replies are dummy fault buffers; no HMAC algorithm is implemented here.
internal static class TransportFaults
{
	public static async Task<int> RunAsync()
	{
		var checks = 0;
		foreach (var mode in new[] { "success", "error", "utf8", "version", "late" })
		{
			Input? input = null;
			Output? output = null;
			try
			{
			// Both test streams are disposed in the unconditional finally below;
			// CA2000 loses this ownership through the scripted callbacks/reflection.
#pragma warning disable CA2000
			input = new Input();
			output = new Output();
#pragma warning restore CA2000
			using var host = Create(input, output);
			using var binding = McwApplicationServices.Bind(host);
			StartReader(host);
			output.OnRequest = request =>
			{
				if (mode != "late") { input.Send(Reply(request, mode)); }
			};
			using var cancellation = new CancellationTokenSource();
			var requestTask = McwApplicationServices.Current.RequestAsync(WalletHmac.Slip21SeedOperation, "SYNTHETIC_REQUEST_MARKER"u8.ToArray(), cancellation.Token);
			var request = await output.Written.Task.WaitAsync(TimeSpan.FromSeconds(2));
			if (mode == "late")
			{
				cancellation.Cancel();
				try { await requestTask; throw new Exception("Transport cancellation was ignored."); }
				catch (OperationCanceledException) { }
				input.Send(Reply(request, "success"));
				await Until(() => input.Frames.Count > 0 && input.Frames.All(IsZero));
				output.OnRequest = next => input.Send(Reply(next, "success"));
				Array.Clear(await McwApplicationServices.Current.RequestAsync(WalletHmac.Slip21SeedOperation, new byte[] { 1 }));
			}
			else if (mode == "success")
			{
				var result = await requestTask;
				if (result.Length != 64 || result.Any(value => value != 0x2a)) { throw new Exception("Successful response ownership was lost."); }
				Array.Clear(result);
			}
			else
			{
				try { await requestTask; throw new Exception("Wire fault was accepted."); }
				catch (IOException error)
				{
					if (error.ToString().Contains("SYNTHETIC_RESPONSE_MARKER", StringComparison.Ordinal)) { throw new Exception("Response bytes entered transport diagnostics."); }
				}
			}
			await Until(() => output.Frames.All(IsZero) && input.Frames.Count > 0 && input.Frames.All(IsZero));
			checks++;
			}
			finally { input?.Dispose(); output?.Dispose(); }
		}

		// Repeated immediate cancellation exercises both sides of delivery races.
		// A canceled wait must eventually clear a result already detached from _pending.
		for (var attempt = 0; attempt < 32; attempt++)
		{
			using var input = new Input();
			using var output = new Output();
			using var host = Create(input, output);
			using var binding = McwApplicationServices.Bind(host);
			StartReader(host);
			using var cancellation = new CancellationTokenSource();
			Task<byte[]>? delivered = null;
			output.OnRequest = request =>
			{
				delivered = PendingResult(host);
				input.Send(Reply(request, "success"));
				cancellation.Cancel();
			};
			var canceled = false;
			try
			{
				Array.Clear(await McwApplicationServices.Current.RequestAsync(WalletHmac.Slip21SeedOperation, new byte[] { 1 }, cancellation.Token));
			}
			catch (OperationCanceledException) { canceled = true; }
			await Until(() => input.Frames.Count > 0 && input.Frames.All(IsZero) && output.Frames.All(IsZero));
			if (canceled && delivered?.IsCompletedSuccessfully == true)
			{
				await Until(() => IsZero(delivered.Result));
			}
			checks++;
		}
		return checks;
	}

	private static bool IsZero(byte[] bytes) => bytes.AsSpan().IndexOfAnyExcept((byte)0) < 0;
	private static async Task Until(Func<bool> predicate)
	{
		var deadline = DateTime.UtcNow.AddSeconds(2);
		while (!predicate())
		{
			if (DateTime.UtcNow > deadline) { throw new Exception("Transport buffer lifetime check timed out."); }
			await Task.Delay(5);
		}
	}
	private static ManagedApplicationHost Create(Stream input, Stream output) =>
		(ManagedApplicationHost)(typeof(ManagedApplicationHost).GetConstructor(BindingFlags.NonPublic | BindingFlags.Instance, null, new[] { typeof(Stream), typeof(Stream) }, null)?.Invoke(new object[] { input, output })
			?? throw new Exception("Actual transport constructor was unavailable."));
	private static void StartReader(ManagedApplicationHost host) =>
		_ = Task.Run(async () => await (Task)(typeof(ManagedApplicationHost).GetMethod("ReadLoopAsync", BindingFlags.NonPublic | BindingFlags.Instance)?.Invoke(host, null)
			?? throw new Exception("Actual transport reader was unavailable.")));
	private static Task<byte[]> PendingResult(ManagedApplicationHost host)
	{
		var pending = (System.Collections.IEnumerable)(typeof(ManagedApplicationHost).GetField("_pending", BindingFlags.NonPublic | BindingFlags.Instance)?.GetValue(host)
			?? throw new Exception("Actual pending map was unavailable."));
		foreach (var item in pending)
		{
			var value = item.GetType().GetProperty("Value")?.GetValue(item) ?? throw new Exception("Pending record was unavailable.");
			return ((TaskCompletionSource<byte[]>)(value.GetType().GetProperty("Completion")?.GetValue(value) ?? throw new Exception("Pending completion was unavailable."))).Task;
		}
		throw new Exception("Request did not enter the actual pending map.");
	}
	private static byte[] Reply(byte[] request, string mode)
	{
		var error = mode is "error" or "utf8";
		var payload = error ? new byte[] { 7, 0 }.Concat(Encoding.ASCII.GetBytes("SYNTHETIC_RESPONSE_MARKER")).ToArray() : Enumerable.Repeat((byte)0x2a, 64).ToArray();
		if (mode == "utf8") { payload[^1] = 0xff; }
		var frame = new byte[20 + payload.Length];
		BinaryPrimitives.WriteInt32LittleEndian(frame, frame.Length - 4);
		BinaryPrimitives.WriteUInt16LittleEndian(frame.AsSpan(4), (ushort)(mode == "version" ? 9 : 1));
		frame[6] = (byte)(error ? 4 : 2);
		request.AsSpan(8, 10).CopyTo(frame.AsSpan(8));
		payload.CopyTo(frame, 20);
		Array.Clear(payload);
		return frame;
	}

	private sealed class Input : Stream
	{
		private readonly Channel<byte[]> _queue = Channel.CreateUnbounded<byte[]>();
		private byte[] _current = Array.Empty<byte>();
		private int _offset;
		public ConcurrentQueue<byte[]> Frames { get; } = new();
		public void Send(byte[] frame) => _queue.Writer.TryWrite(frame);
		public override async ValueTask<int> ReadAsync(Memory<byte> buffer, CancellationToken cancellationToken = default)
		{
			if (_offset == _current.Length) { Array.Clear(_current); _current = await _queue.Reader.ReadAsync(cancellationToken); _offset = 0; }
			if (buffer.Length > 4 && MemoryMarshal.TryGetArray((ReadOnlyMemory<byte>)buffer, out var destination) && destination.Array is { } array) { Frames.Enqueue(array); }
			var count = Math.Min(buffer.Length, _current.Length - _offset);
			_current.AsSpan(_offset, count).CopyTo(buffer.Span); _offset += count;
			return count;
		}
		public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
		public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
		public override bool CanRead => true;
		public override bool CanSeek => false;
		public override bool CanWrite => false;
		public override long Length => throw new NotSupportedException();
		public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
		public override void Flush() { }
		public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
		public override void SetLength(long value) => throw new NotSupportedException();
	}
	private sealed class Output : Stream
	{
		public ConcurrentQueue<byte[]> Frames { get; } = new();
		public Action<byte[]>? OnRequest { get; set; }
		public TaskCompletionSource<byte[]> Written { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
		public override ValueTask WriteAsync(ReadOnlyMemory<byte> buffer, CancellationToken cancellationToken = default)
		{
			if (!MemoryMarshal.TryGetArray(buffer, out var segment) || segment.Array is null) { throw new Exception("Actual write buffer was unavailable."); }
			Frames.Enqueue(segment.Array);
			if (buffer.Span[6] == 3) { var request = buffer.ToArray(); Written.TrySetResult(request); OnRequest?.Invoke(request); }
			return ValueTask.CompletedTask;
		}
		public override void Write(byte[] buffer, int offset, int count) => throw new NotSupportedException();
		public override int Read(byte[] buffer, int offset, int count) => throw new NotSupportedException();
		public override bool CanRead => false;
		public override bool CanSeek => false;
		public override bool CanWrite => true;
		public override long Length => throw new NotSupportedException();
		public override long Position { get => throw new NotSupportedException(); set => throw new NotSupportedException(); }
		public override void Flush() { }
		public override Task FlushAsync(CancellationToken cancellationToken) => Task.CompletedTask;
		public override long Seek(long offset, SeekOrigin origin) => throw new NotSupportedException();
		public override void SetLength(long value) => throw new NotSupportedException();
	}
}
