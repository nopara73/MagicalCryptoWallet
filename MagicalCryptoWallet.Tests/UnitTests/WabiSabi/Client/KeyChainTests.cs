using NBitcoin;
using System.Linq;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.WabiSabi.Client;

public class KeyChainTests
{
	[Fact]
	public void SignTransactionTest()
	{
		var keyManager = KeyManager.CreateNew(out _, "", Network.Main);
		var destinationProvider = new InternalDestinationProvider(keyManager);
		var keyChain = new SyntheticKeyChain(keyManager, "");

		var coinDestination = destinationProvider.GetNextDestinations(1, false).First();
		var coin = new Coin(BitcoinFactory.CreateOutPoint(), new TxOut(Money.Coins(1.0m), coinDestination));

		var transaction = Transaction.Create(Network.Main); // the transaction doesn't contain the input that we request to be signed.

		Assert.Throws<ArgumentException>(() => keyChain.Sign(transaction, coin, transaction.PrecomputeTransactionData()));

		transaction.Inputs.Add(coin.Outpoint);
		var signedTx = keyChain.Sign(transaction, coin, transaction.PrecomputeTransactionData([coin]));
		Assert.True(signedTx.HasWitness);
	}
}
