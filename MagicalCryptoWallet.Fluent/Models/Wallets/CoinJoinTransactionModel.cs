using MagicalCryptoWallet.WabiSabi.Client;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public class CoinJoinTransactionModel : SingleTransactionModel
{
	public override TransactionType Type => TransactionType.Coinjoin;

	public CoinjoinCosts? CoinjoinCosts { get; init; }
}
