using NBitcoin;
using System.Linq;
using MagicalCryptoWallet.Mcw.Crypto;

namespace MagicalCryptoWallet.Crypto;

public class OwnershipIdentifier : IBitcoinSerializable, IEquatable<OwnershipIdentifier>
{
	public const int OwnershipIdLength = 32;
	private byte[] _bytes;

	public OwnershipIdentifier(Key identificationKey, Script scriptPubKey)
	{
		var keyBytes = identificationKey.ToBytes();
		try
		{
			_bytes = WalletHmac.ComputeOwnershipIdentifier(keyBytes, scriptPubKey.ToBytes());
		}
		finally
		{
			Array.Clear(keyBytes);
		}
	}

	public OwnershipIdentifier(byte[] bytes)
	{
		if (bytes.Length != OwnershipIdLength)
		{
			throw new ArgumentException($"Ownership identifier must be {OwnershipIdLength} bytes long.");
		}

		_bytes = bytes.ToArray();
	}

	public OwnershipIdentifier()
		: this(new byte[OwnershipIdLength])
	{
	}

	public byte[] Bytes => _bytes;

	public void ReadWrite(BitcoinStream bitcoinStream)
	{
		bitcoinStream.ReadWrite(_bytes);
	}

	public override int GetHashCode()
	{
		var hash = 0;
		for (var i = 0; i < _bytes.Length; i++)
		{
			hash = (hash << 4) + _bytes[i];
		}
		return hash;
	}

	public static bool operator ==(OwnershipIdentifier? x, OwnershipIdentifier? y) => x?.Equals(y) ?? false;

	public static bool operator !=(OwnershipIdentifier? x, OwnershipIdentifier? y) => !(x == y);

	public override bool Equals(object? other) => Equals(other as OwnershipIdentifier);

	public bool Equals(OwnershipIdentifier? other)
	{
		return other is not null && other.Bytes.SequenceEqual(Bytes);
	}
}
