using System.IO;
using System.Linq;
using System.Reflection;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using Newtonsoft.Json.Linq;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class AutomaticCoinSelectionTests
{
	private const string SyntheticMnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

	[Fact]
	public async Task ImportedWalletIgnoresRemovedSelectionSettingsWithoutChangingKeysAsync()
	{
		var path = Path.Combine(await Common.GetEmptyWorkDirAsync(), "synthetic-wallet.json");
		var original = KeyManager.CreateNew(new Mnemonic(SyntheticMnemonic), "", Network.RegTest, path);
		original.GenerateNewKey("synthetic-label", KeyState.Clean, false);
		original.ToFile();
		var data = JObject.Parse(await File.ReadAllTextAsync(path));
		data["DefaultSendWorkflow"] = "Manual";
		data["ExcludedCoinsFromCoinJoin"] = new JArray(new JObject { ["TransactionId"] = uint256.Zero.ToString(), ["Index"] = 0 });
		await File.WriteAllTextAsync(path, data.ToString());

		var imported = KeyManager.FromFile(path);
		Assert.Equal(original.SegwitExtPubKey, imported.SegwitExtPubKey);
		Assert.Equal(original.TaprootExtPubKey, imported.TaprootExtPubKey);
		Assert.Equal(original.EncryptedSecret, imported.EncryptedSecret);
		Assert.Equal(original.GetKeys().Select(k => (k.PubKey, k.Labels)), imported.GetKeys().Select(k => (k.PubKey, k.Labels)));
		imported.ToFile();
		var saved = JObject.Parse(await File.ReadAllTextAsync(path));
		Assert.Null(saved["DefaultSendWorkflow"]);
		Assert.Null(saved["ExcludedCoinsFromCoinJoin"]);
		Assert.Equal(original.SegwitExtPubKey, KeyManager.FromFile(path).SegwitExtPubKey);
	}



	[Fact]
	public async Task BuildsAndSignsUsingAutomaticWalletInputsAsync()
	{
		await using var app = new SingleWalletTests.SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var keys = KeyManager.CreateNew(new Mnemonic(SyntheticMnemonic), "", Network.RegTest);
		keys.SetFilePath(app.Session.WalletDirectories.NewWalletFilePath);
		var wallet = app.Session.Configure(keys);
		var coins = ServiceFactory.CreateCoins(keys, [("synthetic-funds", 0, 0.02m, true, 1)]);
		foreach (var coin in coins)
		{
			wallet.TransactionProcessor.Process(coin.Transaction);
			wallet.TransactionStore.AddOrUpdate(coin.Transaction);
		}
		await app.InitializeAsync();
		await SingleWalletTests.WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
		using var recipient = new Key();
		var address = recipient.PubKey.GetAddress(ScriptPubKeyType.Segwit, Network.RegTest).ToString();
		Assert.Single(wallet.Coins);
		var signed = WalletOperationTestHelper.SignPayment(app.Session, BitcoinAddress.Create(address, Network.RegTest).ScriptPubKey, Money.Coins(0.005m), "", "synthetic-payment");
		var transaction = signed.Transaction.Transaction;
		Assert.NotEmpty(transaction.Inputs);
		Assert.All(transaction.Inputs, input => Assert.Contains(input.PrevOut, wallet.Coins.Select(c => c.Outpoint)));
		Assert.Contains(transaction.Outputs, output => output.ScriptPubKey == BitcoinAddress.Create(address, Network.RegTest).ScriptPubKey && output.Value == Money.Coins(0.005m));
		var verification = Network.RegTest.CreateTransactionBuilder().AddCoins(coins.Select(c => c.Coin));
		Assert.True(verification.Verify(transaction), "Automatic input selection must produce a valid signed transaction.");

		app.Headers.SetServerTipHeight(new Height.ChainHeight(300));
		var selectedCoin = Assert.Single(wallet.Coins);
		Assert.Equal(coins[0].Transaction.GetConfirmations(300), selectedCoin.Transaction.GetConfirmations(300));
		Assert.Equal(2_000_000, selectedCoin.Amount.Satoshi);
		Assert.Equal("synthetic-funds", selectedCoin.HdPubKey.Labels.ToString());
		coins[0].Transaction.SetUnconfirmed();
		Assert.Equal(0U, selectedCoin.Transaction.GetConfirmations(300));
	}
}
