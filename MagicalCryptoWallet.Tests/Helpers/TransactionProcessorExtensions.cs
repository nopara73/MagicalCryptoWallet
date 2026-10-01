using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionProcessing;

namespace MagicalCryptoWallet.Tests.Helpers;

public static class TransactionProcessorExtensions
{
	public static HdPubKey NewKey(this TransactionProcessor me, string label)
	{
		return me.KeyManager.GenerateNewKey(label, KeyState.Clean, isInternal: true);
	}
}
