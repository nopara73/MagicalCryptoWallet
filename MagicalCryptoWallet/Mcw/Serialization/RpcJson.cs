using System;
using System.Buffers.Binary;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Mcw.Serialization;

/// <summary>JSON-RPC bridge adapter. There is no managed JSON fallback.</summary>
public static class RpcJson
{
	public const int MaxJsonBytes = 8 * 1024 * 1024;
	internal const int MaxTransferBytes = 16 * 1024 * 1024;
	private const int ChunkBytes = 512 * 1024;
	private const int ReadBytes = 256 * 1024;
	private const ushort Open = 0x0100, Append = 0x0101, Finish = 0x0102,
		Read = 0x0103, Close = 0x0104, Numeric = 0x0105;
	internal static readonly UTF8Encoding Utf8 = new(false, true);
	private static readonly SemaphoreSlim Transfers = new(4);

	public static async Task<RpcValue> ParseRpcAsync(string text, CancellationToken cancellationToken = default) =>
		RpcTokens.Decode(await TransformAsync(0, EncodeInput(text), cancellationToken).ConfigureAwait(false));
	public static async Task<RpcValue> ParseRpcAsync(Stream input, CancellationToken cancellationToken = default) =>
		RpcTokens.Decode(await TransformAsync(0, await ReadBoundedAsync(input, cancellationToken).ConfigureAwait(false), cancellationToken).ConfigureAwait(false));
	public static RpcValue ParseRpc(string text) => ParseRpcAsync(text).GetAwaiter().GetResult();
	public static async Task<string> WriteRpcResponseAsync(string? id, RpcValue? result,
		int? errorCode = null, string? errorMessage = null, CancellationToken cancellationToken = default)
	{
		var value = RpcValue.Array([RpcValue.String(id), result, errorCode is { } code ? RpcValue.Create(code) : null, RpcValue.String(errorMessage)]);
		return Utf8.GetString(await TransformAsync(1, RpcTokens.Encode(value), cancellationToken).ConfigureAwait(false));
	}
	internal static async Task<string> WriteRpcPayloadAsync(RpcValue response, CancellationToken cancellationToken = default) =>
		Utf8.GetString(await TransformAsync(1, RpcTokens.Encode(response), cancellationToken).ConfigureAwait(false));
	public static async Task<string> WriteRpcBatchAsync(RpcValue responses, CancellationToken cancellationToken = default) =>
		Utf8.GetString(await TransformAsync(2, RpcTokens.Encode(responses), cancellationToken).ConfigureAwait(false));

	public static Int128 Integer(RpcValue value) => BinaryPrimitives.ReadInt128LittleEndian(ConvertNumber(0, value));
	public static decimal Decimal(RpcValue value)
	{
		var bytes = ConvertNumber(1, value);
		return new decimal([BinaryPrimitives.ReadInt32LittleEndian(bytes), BinaryPrimitives.ReadInt32LittleEndian(bytes.AsSpan(4)),
			BinaryPrimitives.ReadInt32LittleEndian(bytes.AsSpan(8)), BinaryPrimitives.ReadInt32LittleEndian(bytes.AsSpan(12))]);
	}
	public static bool Boolean(RpcValue value) => BinaryPrimitives.ReadInt128LittleEndian(ConvertNumber(2, value)) != 0;
	private static byte[] ConvertNumber(byte mode, RpcValue value)
	{
		var tokens = RpcTokens.Encode(value);
		var request = new byte[tokens.Length + 1]; request[0] = mode; tokens.CopyTo(request, 1);
		var response = RequestAsync(McwApplicationServices.Current, Numeric, request, CancellationToken.None).GetAwaiter().GetResult();
		if (response.Length == 1 && response[0] == 1) { throw new RpcJsonException("JSON number cannot be represented exactly in the requested type."); }
		if (response.Length != 17 || response[0] != 0) { throw new RpcJsonException("Invalid native numeric response."); }
		return response[1..];
	}
	private static byte[] EncodeInput(string text)
	{
		try
		{
			if (text.Length > MaxJsonBytes || Utf8.GetByteCount(text) > MaxJsonBytes) { throw new RpcJsonException("JSON input exceeds the size limit."); }
			return Utf8.GetBytes(text);
		}
		catch (EncoderFallbackException error) { throw new RpcJsonException("JSON input contains an unpaired surrogate.", error); }
	}

	private static async Task<byte[]> TransformAsync(byte action, ReadOnlyMemory<byte> input, CancellationToken cancellationToken)
	{
		if (input.Length > (action == 0 ? MaxJsonBytes : MaxTransferBytes)) { throw new RpcJsonException("JSON transfer exceeds the size limit."); }
		var service = McwApplicationServices.Current;
		using var lifetime = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, service.Stopped);
		lifetime.CancelAfter(TimeSpan.FromSeconds(30));
		var token = lifetime.Token;
		await Transfers.WaitAsync(token).ConfigureAwait(false);
		var begin = new byte[5]; begin[0] = action; BinaryPrimitives.WriteInt32LittleEndian(begin.AsSpan(1), input.Length);
		ulong session = 0;
		try
		{
			var opened = await RequestAsync(service, Open, begin, token).ConfigureAwait(false);
			if (opened.Length != 8 || (session = BinaryPrimitives.ReadUInt64LittleEndian(opened)) == 0) { throw new RpcJsonException("Invalid native JSON transfer identifier."); }
			for (var offset = 0; offset < input.Length;)
			{
				var count = Math.Min(ChunkBytes, input.Length - offset);
				var chunk = new byte[count + 12]; BinaryPrimitives.WriteUInt64LittleEndian(chunk, session);
				BinaryPrimitives.WriteInt32LittleEndian(chunk.AsSpan(8), offset); input.Slice(offset, count).CopyTo(chunk.AsMemory(12));
				var appended = await RequestAsync(service, Append, chunk, token).ConfigureAwait(false);
				offset += count;
				if (appended.Length != 4 || BinaryPrimitives.ReadInt32LittleEndian(appended) != offset) { throw new RpcJsonException("Invalid native JSON transfer offset."); }
			}
			var id = new byte[8]; BinaryPrimitives.WriteUInt64LittleEndian(id, session);
			var finished = await RequestAsync(service, Finish, id, token).ConfigureAwait(false);
			if (finished.Length != 4) { throw new RpcJsonException("Invalid native JSON result length."); }
			var size = BinaryPrimitives.ReadInt32LittleEndian(finished);
			if (size < 0 || size > MaxTransferBytes) { throw new RpcJsonException("Native JSON result exceeds the size limit."); }
			var result = new byte[size];
			for (var offset = 0; offset < size;)
			{
				var maximum = Math.Min(ReadBytes, size - offset);
				var request = new byte[16]; BinaryPrimitives.WriteUInt64LittleEndian(request, session);
				BinaryPrimitives.WriteInt32LittleEndian(request.AsSpan(8), offset); BinaryPrimitives.WriteInt32LittleEndian(request.AsSpan(12), maximum);
				var chunk = await RequestAsync(service, Read, request, token).ConfigureAwait(false);
				if (chunk.Length != maximum) { throw new RpcJsonException("Invalid native JSON result chunk."); }
				chunk.CopyTo(result, offset); offset += chunk.Length;
			}
			session = 0; // Native service removes a result after its last read.
			return result;
		}
		finally
		{
			try
			{
				if (session != 0 && !service.Stopped.IsCancellationRequested)
				{
					var id = new byte[8]; BinaryPrimitives.WriteUInt64LittleEndian(id, session);
					using var cleanup = new CancellationTokenSource(TimeSpan.FromSeconds(2));
					try { await service.RequestAsync(Close, id, cleanup.Token).ConfigureAwait(false); }
					catch (Exception error) when (error is IOException or OperationCanceledException or TimeoutException) { /* Native expiry also releases abandoned transfers. */ }
				}
			}
			finally { Transfers.Release(); }
		}
	}
	private static async Task<byte[]> RequestAsync(IMcwApplicationServices service, ushort operation, ReadOnlyMemory<byte> payload, CancellationToken token)
	{
		try { return await service.RequestAsync(operation, payload, token).ConfigureAwait(false); }
		catch (IOException error) when (error.Message.StartsWith("mcw service error 10:", StringComparison.Ordinal)
			|| error.Message.StartsWith("mcw service error 11:", StringComparison.Ordinal)
			|| error.Message.StartsWith("mcw service error 12:", StringComparison.Ordinal))
		{ throw new RpcJsonException("The native RPC JSON service rejected the operation.", error); }
	}
	public static async Task<byte[]> ReadBoundedAsync(Stream input, CancellationToken cancellationToken = default)
	{
		using var output = new MemoryStream(); var buffer = new byte[64 * 1024];
		while (true)
		{
			var count = await input.ReadAsync(buffer.AsMemory(0, (int)Math.Min(buffer.Length, MaxJsonBytes + 1L - output.Length)), cancellationToken).ConfigureAwait(false);
			if (count == 0) { return output.ToArray(); }
			if (output.Length + count > MaxJsonBytes) { throw new RpcJsonException("JSON input exceeds the size limit."); }
			output.Write(buffer, 0, count);
		}
	}
}
