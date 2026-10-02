using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Mcw.Content;

public enum McwContentCoding : byte { Identity, Gzip, Deflate, Brotli }
public enum McwContentFailure : ushort
{
	MalformedRequest = 1, InvalidEncoding, UnsupportedEncoding, TooManyEncodings,
	Cancelled, Deadline, InputLimit, OutputLimit, ExpansionLimit, WorkLimit,
	AllocationLimit, Truncated, Trailing, Checksum, MalformedContent,
	DictionaryRejected, MetaBlockLimit, AllocationFailed, GzipHeaderLimit,
	GzipMembersLimit, DictionaryLimit
}
public readonly record struct McwContentLayerProof(McwContentCoding Coding, int InputLength, int Consumed, int OutputLength, ulong Work);

public sealed class McwContentDecodingException : IOException
{
	internal McwContentDecodingException(McwContentFailure failure, int layer, ulong input, ulong output)
		: base($"MCW content decoding failed: {failure}.")
	{
		Failure = failure; Layer = layer; InputConsumed = input; OutputProduced = output;
	}
	public McwContentFailure Failure { get; }
	public int Layer { get; }
	public ulong InputConsumed { get; }
	public ulong OutputProduced { get; }
}

public sealed class McwDecodedContent
{
	internal McwDecodedContent(byte[] bytes, int encodedLength, McwContentLayerProof[] layers)
	{ Bytes = bytes; EncodedLength = encodedLength; Layers = Array.AsReadOnly(layers); }
	public byte[] Bytes { get; }
	public int EncodedLength { get; }
	public int DecodedLength => Bytes.Length;
	public IReadOnlyList<McwContentLayerProof> Layers { get; }
	public override string ToString() => $"MCW content: {EncodedLength} encoded bytes, {DecodedLength} decoded bytes, {Layers.Count} layers.";
}

/// <summary>Only the small response decoder crosses this existing host boundary.</summary>
public static class McwContentDecoder
{
	public const ushort Operation = 0x0900;
	public const int MaxBody = 512 * 1024;
	public const int MaxFields = 4;
	public const int MaxField = 2048;
	public const ushort TimeoutMilliseconds = 5000;
	private const ushort Version = 1;
	private const ulong MaxWork = 16_000_000;

	public static async Task<McwDecodedContent> DecodeAsync(ReadOnlyMemory<byte> encoded,
		IReadOnlyList<string> encodings, IMcwApplicationServices services,
		CancellationToken cancellationToken = default)
	{
		ArgumentNullException.ThrowIfNull(encodings);
		ArgumentNullException.ThrowIfNull(services);
		cancellationToken.ThrowIfCancellationRequested();
		services.Stopped.ThrowIfCancellationRequested();
		if (encoded.Length > MaxBody || encodings.Count > MaxFields)
		{ throw new IOException("The bounded content request exceeds its limit."); }
		var expected = ParseCodings(encodings);
		var size = 10 + encoded.Length;
		foreach (var field in encodings)
		{
			if (field is null || field.Length > MaxField) { throw new IOException("Invalid content encoding field."); }
			size = checked(size + 2 + field.Length);
		}
		var request = new byte[size];
		BinaryPrimitives.WriteUInt16LittleEndian(request, Version);
		BinaryPrimitives.WriteUInt16LittleEndian(request.AsSpan(2), TimeoutMilliseconds);
		request[4] = (byte)encodings.Count;
		BinaryPrimitives.WriteInt32LittleEndian(request.AsSpan(6), encoded.Length);
		var offset = 10;
		foreach (var field in encodings)
		{
			BinaryPrimitives.WriteUInt16LittleEndian(request.AsSpan(offset), (ushort)field.Length); offset += 2;
			foreach (var character in field)
			{
				if (character > 127) { throw new IOException("Invalid content encoding field."); }
				request[offset++] = (byte)character;
			}
		}
		encoded.CopyTo(request.AsMemory(offset));
		using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, services.Stopped);
		var reply = await services.RequestAsync(Operation, request, linked.Token).ConfigureAwait(false);
		linked.Token.ThrowIfCancellationRequested();
		return ParseReply(reply, encoded, expected);
	}

	private static McwContentCoding[] ParseCodings(IReadOnlyList<string> values)
	{
		var codes = new List<McwContentCoding>(MaxFields);
		foreach (var value in values)
		{
			if (value is null || value.Length > MaxField) { throw new IOException("Invalid content encoding field."); }
			foreach (var item in value.Split(','))
			{
				var token = item.Trim(' ', '\t');
				var code = token.Equals("identity", StringComparison.OrdinalIgnoreCase) ? McwContentCoding.Identity
					: token.Equals("gzip", StringComparison.OrdinalIgnoreCase) || token.Equals("x-gzip", StringComparison.OrdinalIgnoreCase) ? McwContentCoding.Gzip
					: token.Equals("deflate", StringComparison.OrdinalIgnoreCase) ? McwContentCoding.Deflate
					: token.Equals("br", StringComparison.OrdinalIgnoreCase) ? McwContentCoding.Brotli
					: (McwContentCoding)byte.MaxValue;
				// Native decoding remains authoritative for malformed/unsupported fields.
				// An unrecognized token cannot validate a success proof.
				if (codes.Count == MaxFields) { throw new IOException("Too many content encodings."); }
				codes.Add(code);
			}
		}
		codes.Reverse(); return codes.ToArray();
	}

	private static McwDecodedContent ParseReply(byte[] reply, ReadOnlyMemory<byte> encoded, McwContentCoding[] expected)
	{
		static IOException Invalid() => new("Invalid MCW content response.");
		if (reply.Length < 3 || BinaryPrimitives.ReadUInt16LittleEndian(reply) != Version) { throw Invalid(); }
		if (reply[2] == 1)
		{
			if (reply.Length != 22) { throw Invalid(); }
			var failure = (McwContentFailure)BinaryPrimitives.ReadUInt16LittleEndian(reply.AsSpan(3));
			var input = BinaryPrimitives.ReadUInt64LittleEndian(reply.AsSpan(6));
			var output = BinaryPrimitives.ReadUInt64LittleEndian(reply.AsSpan(14));
			if (!Enum.IsDefined(failure) || reply[5] >= MaxFields || input > MaxBody || output > MaxBody) { throw Invalid(); }
			throw new McwContentDecodingException(failure, reply[5], input, output);
		}
		if (reply[2] != 0 || reply.Length < 12) { throw Invalid(); }
		var inputLength = BinaryPrimitives.ReadInt32LittleEndian(reply.AsSpan(3));
		var outputLength = BinaryPrimitives.ReadInt32LittleEndian(reply.AsSpan(7));
		var count = reply[11];
		if (inputLength != encoded.Length || outputLength is < 0 or > MaxBody || count != expected.Length
			|| count > MaxFields || reply.Length != 12 + count * 21 + outputLength) { throw Invalid(); }
		var layers = new McwContentLayerProof[count];
		var offset = 12; var previous = inputLength; ulong work = 0;
		for (var i = 0; i < count; i++, offset += 21)
		{
			var coding = (McwContentCoding)reply[offset];
			var input = BinaryPrimitives.ReadInt32LittleEndian(reply.AsSpan(offset + 1));
			var consumed = BinaryPrimitives.ReadInt32LittleEndian(reply.AsSpan(offset + 5));
			var output = BinaryPrimitives.ReadInt32LittleEndian(reply.AsSpan(offset + 9));
			var spent = BinaryPrimitives.ReadUInt64LittleEndian(reply.AsSpan(offset + 13));
			if (!Enum.IsDefined(coding) || coding != expected[i] || input != previous || consumed != input
				|| output is < 0 or > MaxBody || spent > MaxWork || work > MaxWork - spent) { throw Invalid(); }
			work += spent; previous = output; layers[i] = new(coding, input, consumed, output, spent);
		}
		if (previous != outputLength) { throw Invalid(); }
		var bytes = reply.AsSpan(offset, outputLength).ToArray();
		if (count == 0 && !bytes.AsSpan().SequenceEqual(encoded.Span)) { throw Invalid(); }
		return new(bytes, inputLength, layers);
	}
}
