using System;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin.Secp256k1;
using NNostr.Client;

namespace MagicalCryptoWallet.Mcw.Nostr;

/// <summary>Canonical event hashing belongs to mcw; signature verification remains managed.</summary>
public static class McwNostrEventId
{
	public const ushort Operation = 0x0C00;
	private const int MaxRequestBytes = 1_048_560;
	// NNostr hashes UTF-8 with replacement for malformed managed UTF-16 strings.
	private static readonly UTF8Encoding Utf8 = new(false, false);

	public static bool IsAuthentic(NostrEvent note)
	{
		ArgumentNullException.ThrowIfNull(note);
		var services = McwApplicationServices.Current;
		var digest = ComputeDigestAsync(note, services.Stopped).GetAwaiter().GetResult();
		if (!string.Equals(note.Id, Convert.ToHexStringLower(digest), StringComparison.Ordinal))
		{
			return false;
		}

		// Verify the Rust digest directly. NNostr.Verify() would hash the event again.
		var publicKey = Context.Instance.CreateXOnlyPubKey(Convert.FromHexString(note.PublicKey));
		return SecpSchnorrSignature.TryCreate(Convert.FromHexString(note.Signature), out var signature)
			&& publicKey.SigVerifyBIP340(signature, digest);
	}

	public static async Task<byte[]> ComputeDigestAsync(NostrEvent note, CancellationToken cancellationToken = default)
	{
		ArgumentNullException.ThrowIfNull(note);
		var result = await McwApplicationServices.Current.RequestAsync(Operation, Encode(note), cancellationToken).ConfigureAwait(false);
		if (result.Length != 32)
		{
			throw new InvalidDataException("Invalid mcw Nostr digest response.");
		}
		return result;
	}

	private static byte[] Encode(NostrEvent note)
	{
		var publicKey = Convert.FromHexString(note.PublicKey);
		if (publicKey.Length != 32 || !string.Equals(note.PublicKey, Convert.ToHexStringLower(publicKey), StringComparison.Ordinal))
		{
			throw new ArgumentException("Nostr public key must be 32 bytes of lowercase hexadecimal.", nameof(note));
		}
		using var stream = new MemoryStream();
		using var writer = new BinaryWriter(stream, Utf8, leaveOpen: true);
		writer.Write((byte)1);
		writer.Write(publicKey);
		writer.Write(note.CreatedAt?.ToUnixTimeSeconds() ?? 0);
		writer.Write(note.Kind);
		writer.Write(checked((uint)note.Tags.Count));
		foreach (var tag in note.Tags)
		{
			EnsureRoom(stream, 4);
			var hasName = tag.TagIdentifier is not null;
			writer.Write(checked((uint)tag.Data.Count + (hasName ? 1U : 0U)));
			if (hasName)
			{
				WriteText(writer, stream, tag.TagIdentifier!);
			}
			foreach (var item in tag.Data)
			{
				WriteText(writer, stream, item ?? string.Empty);
			}
		}
		WriteText(writer, stream, note.Content ?? string.Empty);
		writer.Flush();
		return stream.ToArray();
	}

	private static void WriteText(BinaryWriter writer, MemoryStream stream, string text)
	{
		var length = Utf8.GetByteCount(text);
		EnsureRoom(stream, 4L + length);
		writer.Write(checked((uint)length));
		writer.Write(Utf8.GetBytes(text));
	}

	private static void EnsureRoom(MemoryStream stream, long bytes)
	{
		if (bytes > MaxRequestBytes - stream.Length)
		{
			throw new ArgumentException("Nostr event exceeds the application request limit.");
		}
	}
}
