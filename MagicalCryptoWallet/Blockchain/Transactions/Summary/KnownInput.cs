using NBitcoin;

namespace MagicalCryptoWallet.Blockchain.Transactions.Summary;

public class KnownInput : IInput
{
	public KnownInput(Money amount)
	{
		Amount = amount;
	}

	public virtual Money? Amount { get; }
}
