using NBitcoin;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public abstract class SingleTransactionModel : TransactionModel
{
	public required uint Confirmations { get; init; }

	public required Func<string> HexFunction { get; init; }
	public Lazy<string> Hex => new(HexFunction());

	public FeeRate? FeeRate { get; init; }
}
