using MagicalCryptoWallet.Crypto;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.WabiSabi.Client;

public class KeyChain : IKeyChain
{
	public KeyChain(WalletAuthorization authorization) { _authorization = authorization; }
	private readonly WalletAuthorization _authorization;
	private Key GetMasterKey() => _authorization.MasterKey.PrivateKey;

	public OwnershipProof GetOwnershipProof(IDestination destination, CoinJoinInputCommitmentData commitmentData)
	{
		Key key = _authorization.GetSecrets(destination.ScriptPubKey).SingleOrDefault()
			?? throw new InvalidOperationException($"The signing key for '{destination.ScriptPubKey}' was not found.");
		Key masterKey = GetMasterKey();
		BitcoinSecret secret = key.GetBitcoinSecret(_authorization.KeyManager.GetNetwork(), destination.ScriptPubKey);

		return NBitcoinExtensions.GetOwnershipProof(masterKey, secret, destination.ScriptPubKey, commitmentData);
	}

	public Transaction Sign(Transaction transaction, Coin coin, PrecomputedTransactionData precomputedTransactionData)
	{
		transaction = transaction.Clone();

		if (transaction.Inputs.Count == 0)
		{
			throw new ArgumentException("No inputs to sign.", nameof(transaction));
		}

		var txInput = transaction.Inputs.AsIndexedInputs().FirstOrDefault(input => input.PrevOut == coin.Outpoint)
			?? throw new InvalidOperationException("Missing input.");
		Key key = _authorization.GetSecrets(coin.ScriptPubKey).SingleOrDefault()
			?? throw new InvalidOperationException($"The signing key for '{coin.ScriptPubKey}' was not found.");
		BitcoinSecret secret = key.GetBitcoinSecret(_authorization.KeyManager.GetNetwork(), coin.ScriptPubKey);

		TransactionBuilder builder = Network.Main.CreateTransactionBuilder();
		builder.AddKeys(secret);
		builder.AddCoins(coin);
		builder.SetSigningOptions(new SigningOptions(TaprootSigHash.All, (TaprootReadyPrecomputedTransactionData)precomputedTransactionData));
		builder.SignTransactionInPlace(transaction);

		return transaction;
	}
}
