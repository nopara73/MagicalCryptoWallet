using NBitcoin;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Crypto;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Tests.Helpers;

/// <summary>Test participants authorize each operation with synthetic credentials; no fixture retains private keys.</summary>
public sealed class SyntheticKeyChain(KeyManager keys, string password = "") : IKeyChain
{
	public OwnershipProof GetOwnershipProof(IDestination destination, CoinJoinInputCommitmentData committedData)
	{
		using var scope = WalletAuthorization.Create(keys, password);
		return new KeyChain(scope).GetOwnershipProof(destination, committedData);
	}
	public Transaction Sign(Transaction transaction, Coin coin, PrecomputedTransactionData precomputeTransactionData)
	{
		using var scope = WalletAuthorization.Create(keys, password);
		return new KeyChain(scope).Sign(transaction, coin, precomputeTransactionData);
	}
}
