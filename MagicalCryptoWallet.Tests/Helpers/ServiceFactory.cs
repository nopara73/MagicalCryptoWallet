using NBitcoin;
using System.Collections.Generic;
using System.Linq;
using MagicalCryptoWallet.Blockchain.BlockFilters;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Blockchain.Transactions;

namespace MagicalCryptoWallet.Tests.Helpers;

public static class ServiceFactory
{
	public static TransactionFactory CreateTransactionFactory(
		(string Label, int KeyIndex, decimal Amount, bool Confirmed, int AnonymitySet)[] coins)
	{
		string password = "foo";
		KeyManager keyManager = CreateKeyManager(password);
		SmartCoin[] sCoins = CreateCoins(keyManager, coins);
		var coinsView = new CoinsView(sCoins);
#pragma warning disable CA2000 // Dispose objects before losing scope - test helper, ownership transferred to TransactionFactory
		var mockTransactionStore = new AllTransactionStore(".", Network.Main);
#pragma warning restore CA2000
		return new TransactionFactory(Network.Main, keyManager, coinsView, mockTransactionStore, password);
	}

	public static SmartCoin[] CreateCoins(
		KeyManager keyManager,
		(string Label, int KeyIndex, decimal Amount, bool Confirmed, int AnonymitySet)[] coins)
	{
		var generated = keyManager.GetKeys().Length;
		var toGenerate = coins.Length - generated;
		for (int i = 0; i < toGenerate; i++)
		{
			keyManager.GenerateNewKey("", KeyState.Clean, false);
		}

		var keys = keyManager.GetKeys().Take(coins.Length).ToArray();
		var sCoins = new List<SmartCoin>(coins.Length);

		foreach (var c in coins)
		{
			var k = keys[c.KeyIndex];
			k.SetLabel(c.Label);

			var sCoin = BitcoinFactory.CreateSmartCoin(keys[c.KeyIndex], c.Amount, c.Confirmed, c.AnonymitySet);
			sCoin.SetAnonymitySet(c.AnonymitySet);

			sCoins.Add(sCoin);
		}

		foreach (var coin in sCoins)
		{
			foreach (var sameLabelCoin in sCoins.Where(c => !c.HdPubKey.Labels.IsEmpty && c.HdPubKey.Labels == coin.HdPubKey.Labels))
			{
				sameLabelCoin.HdPubKey.Cluster = coin.HdPubKey.Cluster;
			}
		}

		var uniqueCoins = sCoins.Distinct().Count();
		if (uniqueCoins != sCoins.Count)
		{
			throw new InvalidOperationException($"Coin clones have been detected. Number of all coins:{sCoins.Count}, unique coins:{uniqueCoins}.");
		}

		return sCoins.ToArray();
	}

	public static KeyManager CreateKeyManager(string password = "blahblahblah", bool isTaprootAllowed = false, Mnemonic? mnemonic = null)
	{
		mnemonic ??= new Mnemonic(Wordlist.English, WordCount.Twelve);
		ExtKey extKey = mnemonic.DeriveExtKey(password);
		var encryptedSecret = extKey.PrivateKey.GetEncryptedBitcoinSecret(password, Network.Main);

		HDFingerprint masterFingerprint = extKey.Neuter().PubKey.GetHDFingerPrint();
		BlockchainState blockchainState = new(Network.Main);
		KeyPath segwitAccountKeyPath = KeyManager.GetAccountKeyPath(Network.Main, ScriptPubKeyType.Segwit);
		ExtPubKey segwitExtPubKey = extKey.Derive(segwitAccountKeyPath).Neuter();

		ExtPubKey? taprootExtPubKey = null;
		if (isTaprootAllowed)
		{
			KeyPath taprootAccountKeyPath = KeyManager.GetAccountKeyPath(Network.Main, ScriptPubKeyType.TaprootBIP86);
			taprootExtPubKey = extKey.Derive(taprootAccountKeyPath).Neuter();
		}

		return new KeyManager(encryptedSecret, extKey.ChainCode, masterFingerprint, segwitExtPubKey, taprootExtPubKey, 21, blockchainState, null, segwitAccountKeyPath, null);
	}

}
