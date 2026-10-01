using MagicalCryptoWallet.Blockchain.Transactions;

namespace MagicalCryptoWallet.Tests.UnitTests.Extensions;

public static class SmartTransactionExtensions
{
	public static bool IsRBF(this SmartTransaction tx) => !tx.Confirmed;
}
