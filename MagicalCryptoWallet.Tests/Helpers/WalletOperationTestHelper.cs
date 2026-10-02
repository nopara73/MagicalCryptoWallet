using NBitcoin;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Tests.Helpers;

internal static class WalletOperationTestHelper
{
	public static BuildTransactionResult SignPayment(WalletSession session, Script destination, Money amount, string password, string label = "synthetic")
	{
		session.EnsureReady();
		var wallet = session.GetWallet()!;
		using var authorization = WalletAuthorization.Create(wallet.KeyManager, password);
		var preview = wallet.BuildTransaction(new Destination(destination), amount, label, new FeeRate(2m), wallet.Coins, subtractFee: false);
		var result = authorization.Sign(preview);
		session.CompleteOperationAuthorization(authorization);
		return result;
	}
}
