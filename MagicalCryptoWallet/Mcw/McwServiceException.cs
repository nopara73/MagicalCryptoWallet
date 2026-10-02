using System.IO;

namespace MagicalCryptoWallet.Mcw;

/// <summary>A typed native-service rejection without untrusted diagnostic text.</summary>
public sealed class McwServiceException(ushort operation, ushort code)
	: IOException($"mcw service error {code}.")
{
	public ushort Operation { get; } = operation;
	public ushort Code { get; } = code;
}
