using System;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Mcw.Scripts;

/// <summary>The explicit client adapter for the retained legacy Script text format.</summary>
public static class ScriptTextClient
{
	public const ushort ParseOperation = 0x0D08;
	public const ushort RenderOperation = 0x0D09;
	// The version-one mcw frame's 1 MiB bound minus its 16-byte header.
	public const int MaximumPayloadBytes = 1_048_560;
	private static readonly UTF8Encoding Utf8 = new(false, true);

	public static byte[] Parse(string text, CancellationToken cancellationToken = default) =>
		ParseAsync(text, cancellationToken).GetAwaiter().GetResult();

	public static async Task<byte[]> ParseAsync(string text, CancellationToken cancellationToken = default)
	{
		ArgumentNullException.ThrowIfNull(text);
		var length = Utf8.GetByteCount(text);
		CheckLength(length);
		var result = await RequestAsync(ParseOperation, Utf8.GetBytes(text), cancellationToken).ConfigureAwait(false);
		CheckLength(result.Length);
		return result;
	}

	public static string Render(ReadOnlyMemory<byte> script, CancellationToken cancellationToken = default) =>
		RenderAsync(script, cancellationToken).GetAwaiter().GetResult();

	public static async Task<string> RenderAsync(ReadOnlyMemory<byte> script, CancellationToken cancellationToken = default)
	{
		CheckLength(script.Length);
		var result = await RequestAsync(RenderOperation, script, cancellationToken).ConfigureAwait(false);
		CheckLength(result.Length);
		try
		{
			return Utf8.GetString(result);
		}
		catch (DecoderFallbackException error)
		{
			throw new FormatException("The mcw Script text response is not valid UTF-8.", error);
		}
	}

	private static Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken)
	{
		cancellationToken.ThrowIfCancellationRequested();
		return McwApplicationServices.Current.RequestAsync(operation, payload, cancellationToken);
	}

	private static void CheckLength(int length)
	{
		if (length > MaximumPayloadBytes)
		{
			throw new FormatException("The mcw Script text payload exceeds the protocol limit.");
		}
	}
}
