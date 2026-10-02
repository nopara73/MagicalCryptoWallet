using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class AmountExtensions
{
	public static double Diff(this Amount self, Amount toCompare)
	{
		return ((double)self.Btc.Satoshi / toCompare.Btc.Satoshi) - 1;
	}
}
