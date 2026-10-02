using System;
using System.Buffers;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.IO;
using System.IO.Pipelines;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Tor.Control;
using MagicalCryptoWallet.Tor.Control.Exceptions;
using MagicalCryptoWallet.Tor.Control.Messages;

namespace MagicalCryptoWallet.Mcw.Privacy;

/// <summary>Pipe I/O and typed bridge translation; Tor grammar is owned by Rust.</summary>
public static class McwTorControlCodec
{
	private const ushort ReplyOperation = 0x0f00;
	private const ushort LineOperation = 0x0f01;
	private const int MaxInput = 524288;
	private const int MaxLine = 65536;
	private const int MaxLines = 16384;
	private const int MaxResponse = MaxInput + 4 * MaxLines + 16;

	public static async Task<TorControlReply> ReadReplyAsync(PipeReader reader, CancellationToken cancellationToken)
	{
		var result = await ReadAsync(reader, ReplyOperation, cancellationToken).ConfigureAwait(false);
		return new TorControlReply((StatusCode)result.Status, result.Lines);
	}

	public static async ValueTask<string> ReadLineAsync(PipeReader reader, CancellationToken cancellationToken = default)
	{
		var result = await ReadAsync(reader, LineOperation, cancellationToken).ConfigureAwait(false);
		return result.Lines[0];
	}

	private sealed record Result(int Consumed, int Status, List<string> Lines);

	private static async Task<Result> ReadAsync(PipeReader reader, ushort operation, CancellationToken cancellationToken)
	{
		ArgumentNullException.ThrowIfNull(reader);
		cancellationToken.ThrowIfCancellationRequested();
		var service = McwApplicationServices.Current;
		using var stop = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, service.Stopped);
		var pending = new ArrayBufferWriter<byte>();
		while (true)
		{
			var read = await reader.ReadAsync(stop.Token).ConfigureAwait(false);
			var buffer = read.Buffer;
			var consumed = buffer.Start;
			var examined = buffer.End;
			try
			{
				if (read.IsCanceled) { throw new OperationCanceledException(stop.Token); }
				var length = (int)Math.Min(buffer.Length, MaxInput - pending.WrittenCount);
				var total = pending.WrittenCount + length;
				var eof = read.IsCompleted && buffer.Length == length;
				var payload = new byte[total + 1];
				payload[0] = eof ? (byte)1 : (byte)0;
				pending.WrittenSpan.CopyTo(payload.AsSpan(1));
				buffer.Slice(0, length).CopyTo(payload.AsSpan(1 + pending.WrittenCount));
				var response = await service.RequestAsync(operation, payload, stop.Token).ConfigureAwait(false);
				stop.Token.ThrowIfCancellationRequested();
				var result = Decode(response, operation, total);
				if (result is not null)
				{
					if (result.Consumed <= pending.WrittenCount) { throw InvalidResponse(); }
					consumed = buffer.GetPosition(result.Consumed - pending.WrittenCount);
					examined = consumed;
					return result;
				}
				if (eof || total == MaxInput) { throw InvalidResponse(); }
				// Release incomplete bytes to avoid producer backpressure deadlock.
				// This buffer holds raw bytes only; the Rust result owns all grammar.
				pending.Write(payload.AsSpan(1 + pending.WrittenCount, length));
				consumed = buffer.GetPosition(length);
			}
			finally
			{
				reader.AdvanceTo(consumed, examined);
			}
		}
	}

	private static Result? Decode(byte[] response, ushort operation, int available)
	{
		if (response.Length is 0 or > MaxResponse) { throw InvalidResponse(); }
		if (response[0] == 0)
		{
			if (response.Length != 1) { throw InvalidResponse(); }
			return null;
		}
		if (response[0] == 2) { ThrowParserError(response, operation); }
		if (response[0] != 1 || response.Length < 13) { throw InvalidResponse(); }
		var consumed = BinaryPrimitives.ReadUInt32LittleEndian(response.AsSpan(1));
		var status = BinaryPrimitives.ReadInt32LittleEndian(response.AsSpan(5));
		var count = BinaryPrimitives.ReadUInt32LittleEndian(response.AsSpan(9));
		if (consumed == 0 || consumed > available || count is 0 or > MaxLines
			|| (operation == LineOperation && (status != 0 || count != 1))) { throw InvalidResponse(); }
		var lines = new List<string>((int)count);
		var offset = 13;
		for (var i = 0; i < count; i++)
		{
			if (response.Length - offset < 4) { throw InvalidResponse(); }
			var length = BinaryPrimitives.ReadUInt32LittleEndian(response.AsSpan(offset));
			offset += 4;
			if (length > MaxLine || length > response.Length - offset) { throw InvalidResponse(); }
			var text = response.AsSpan(offset, (int)length);
			foreach (var octet in text) { if (octet > 127) { throw InvalidResponse(); } }
			lines.Add(Encoding.ASCII.GetString(text));
			offset += (int)length;
		}
		if (offset != response.Length) { throw InvalidResponse(); }
		return new Result((int)consumed, status, lines);
	}

	private static void ThrowParserError(byte[] response, ushort operation)
	{
		if (response.Length < 2) { throw InvalidResponse(); }
		var category = response[1];
		var expectedLength = category switch { 2 => 3, 4 => 5, _ => 2 };
		if (response.Length != expectedLength || category > 5
			|| (operation == LineOperation && category is 2 or 3 or 4)) { throw InvalidResponse(); }
		switch (category)
		{
			case 0: throw new InvalidDataException("No more data.");
			case 1: throw new InvalidDataException("Incomplete message.");
			case 2:
				if (response[2] > 1) { throw InvalidResponse(); }
				throw new TorControlReplyParseException("No reply line was received.",
					new InvalidDataException(response[2] == 1 ? "Incomplete message." : "No more data."));
			case 3: throw new TorControlReplyParseException("Status code requires at least 3 characters.");
			case 4:
				foreach (var octet in response.AsSpan(2)) { if (octet > 127) { throw InvalidResponse(); } }
				throw new TorControlReplyParseException($"Unknown status code: '{Encoding.ASCII.GetString(response, 2, 3)}'.");
			default:
				if (operation == ReplyOperation) { throw new TorControlReplyParseException("Tor control parsing limit exceeded."); }
				throw new InvalidDataException("Tor control parsing limit exceeded.");
		}
	}

	private static IOException InvalidResponse() => new("Invalid mcw Tor control codec response.");
}
