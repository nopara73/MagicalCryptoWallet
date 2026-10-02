using System;
using System.IO;
using System.Threading;

namespace MagicalCryptoWallet.Mcw.Crypto;

/// <summary>Typed adapters for three retained hash computations; owns no wallet key or node.</summary>
public static class WalletHmac
{
	public const ushort OwnershipOperation = 0x0A10;
	public const ushort Slip21SeedOperation = 0x0A11;
	public const ushort Slip21ChildOperation = 0x0A12;
	public const int MaxRequestBytes = 1_048_560;
	private const int KeyBytes = 32;
	private const int NodeBytes = 64;

	public static byte[] ComputeOwnershipIdentifier(ReadOnlySpan<byte> key, ReadOnlySpan<byte> script, CancellationToken cancellationToken = default) =>
		KeyedRequest(OwnershipOperation, key, script, KeyBytes, cancellationToken);

	public static byte[] DeriveSlip21Seed(ReadOnlySpan<byte> seed, CancellationToken cancellationToken = default)
	{
		cancellationToken.ThrowIfCancellationRequested();
		CheckLength(0, seed.Length);
		return Request(Slip21SeedOperation, seed.ToArray(), NodeBytes, cancellationToken);
	}

	public static byte[] DeriveSlip21Child(ReadOnlySpan<byte> parentKey, ReadOnlySpan<byte> label, CancellationToken cancellationToken = default) =>
		KeyedRequest(Slip21ChildOperation, parentKey, label, NodeBytes, cancellationToken);

	private static byte[] KeyedRequest(ushort operation, ReadOnlySpan<byte> key, ReadOnlySpan<byte> message, int responseBytes, CancellationToken cancellationToken)
	{
		cancellationToken.ThrowIfCancellationRequested();
		if (key.Length != KeyBytes)
		{
			throw new ArgumentException("Invalid wallet hash key.", nameof(key));
		}
		CheckLength(KeyBytes, message.Length);
		var payload = new byte[KeyBytes + message.Length];
		key.CopyTo(payload);
		message.CopyTo(payload.AsSpan(KeyBytes));
		return Request(operation, payload, responseBytes, cancellationToken);
	}

	private static void CheckLength(int prefix, int length)
	{
		if (length > MaxRequestBytes - prefix)
		{
			throw new ArgumentException("Wallet hash request exceeds the limit.");
		}
	}

	private static byte[] Request(ushort operation, byte[] payload, int responseBytes, CancellationToken cancellationToken)
	{
		try
		{
			var service = McwApplicationServices.Current;
			using var lifetime = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, service.Stopped);
			lifetime.Token.ThrowIfCancellationRequested();
			// The host reader runs independently and never resumes on the UI context.
			// Existing synchronous domain APIs retain their signatures and ownership.
			var response = service.RequestAsync(operation, payload, lifetime.Token).GetAwaiter().GetResult();
			if (response.Length != responseBytes)
			{
				Array.Clear(response);
				throw new IOException("Invalid wallet hash response.");
			}
			return response;
		}
		finally
		{
			// The transport must also clear its copies, including canceled late replies.
			Array.Clear(payload);
		}
	}
}
