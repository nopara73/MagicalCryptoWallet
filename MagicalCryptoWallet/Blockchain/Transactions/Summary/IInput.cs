using NBitcoin;

namespace MagicalCryptoWallet.Blockchain.Transactions.Summary;

public interface IInput
{
	Money? Amount { get; }
}
