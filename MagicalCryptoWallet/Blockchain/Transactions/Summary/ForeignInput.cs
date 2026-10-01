using NBitcoin;

namespace MagicalCryptoWallet.Blockchain.Transactions.Summary;

public class ForeignInput : IInput
{
	public Money? Amount => default;
}
