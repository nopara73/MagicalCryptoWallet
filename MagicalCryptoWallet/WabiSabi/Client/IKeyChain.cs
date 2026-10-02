using MagicalCryptoWallet.Crypto;

namespace MagicalCryptoWallet.WabiSabi.Client;

public interface IKeyChain
{
	OwnershipProof GetOwnershipProof(IDestination destination, CoinJoinInputCommitmentData committedData);

	Transaction Sign(Transaction transaction, Coin coin, PrecomputedTransactionData precomputeTransactionData);
}
