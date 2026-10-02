using System.Text;
using NBitcoin;
using MagicalCryptoWallet.Mcw.Crypto;

namespace MagicalCryptoWallet.Crypto;

public class Slip21Node
{
	private static readonly int KEY_SIZE = 32;
	private byte[] _data;

	public Slip21Node(byte[] data)
	{
		_data = data.Length == 64
			? data
			: throw new ArgumentException("The data array has to be 64 bytes long.", nameof(data));
	}

	public Key Key => new(_data[KEY_SIZE..]);

	public static Slip21Node FromSeed(byte[] seed)
	{
		ArgumentNullException.ThrowIfNull(seed);
		return new(WalletHmac.DeriveSlip21Seed(seed));
	}

	public Slip21Node DeriveChild(string label) =>
		DeriveChild(Encoding.ASCII.GetBytes(label));

	public Slip21Node DeriveChild(byte[] label)
	{
		ArgumentNullException.ThrowIfNull(label);
		return new(WalletHmac.DeriveSlip21Child(_data.AsSpan(0, KEY_SIZE), label));
	}
}
