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
using MagicalCryptoWallet.Client.Rpc;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Rpc;
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

	[Theory]
	[InlineData("build")]
	[InlineData("buildunsafetransaction")]
	[InlineData("send")]
	public async Task RpcRejectsExplicitInputsAndKeepsPaymentFeeAndPasswordParametersAsync(string method)
	{
		var service = new MagicalCryptoWalletJsonRpcService(null!);
		var provider = new JsonRpcServiceMetadataProvider(typeof(MagicalCryptoWalletJsonRpcService));
		Assert.True(provider.TryGetMetadata(method, out var metadata));
		Assert.Equal(new[] { "payments", "feeTarget", "feeRate", "password" }, metadata.Parameters.Select(p => p.name));
		var handler = new JsonRpcRequestHandler<MagicalCryptoWalletJsonRpcService>(service, Network.RegTest);
		foreach (var parameters in new[] { """{"payments":[],"coins":[],"feeRate":2}""", """[[],[],null,2,""]""" })
		{
			var response = JObject.Parse(await handler.HandleAsync("/", $$$"""{"jsonrpc":"2.0","id":1,"method":"{{{method}}}","params":{{{parameters}}}}""", CancellationToken.None));
			Assert.Equal((int)JsonRpcErrorCodes.InvalidParams, response["error"]!["code"]!.Value<int>());
		}
	}

	[Fact]
	public async Task RemovedRpcExclusionMethodIsUnavailableAsync()
	{
		var handler = new JsonRpcRequestHandler<MagicalCryptoWalletJsonRpcService>(new MagicalCryptoWalletJsonRpcService(null!), Network.RegTest);
		var response = JObject.Parse(await handler.HandleAsync("/", """{"jsonrpc":"2.0","id":1,"method":"excludefromcoinjoin","params":[]}""", CancellationToken.None));
		Assert.Equal((int)JsonRpcErrorCodes.MethodNotFound, response["error"]!["code"]!.Value<int>());
	}

	[Theory]
	[InlineData("build")]
	[InlineData("buildunsafetransaction")]
	public async Task RpcBuildsAndSignsUsingAutomaticWalletInputsAsync(string method)
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
		var service = new MagicalCryptoWalletJsonRpcService(app.Global);
		Assert.Single(service.GetUnspentCoinList());
		Assert.DoesNotContain(service.GetUnspentCoinList(), result => result.ContainsKey("excludedFromCoinjoin"));
		var handler = new JsonRpcRequestHandler<MagicalCryptoWalletJsonRpcService>(service, Network.RegTest);
		var response = JObject.Parse(await handler.HandleAsync("/", $$$"""{"jsonrpc":"2.0","id":1,"method":"{{{method}}}","params":{"payments":[{"sendto":"{{{address}}}","amount":500000,"label":"synthetic-payment"}],"feeRate":2}}""", CancellationToken.None));
		Assert.True(response["error"] is null, response.ToString());
		var transaction = Transaction.Parse(response["result"]!.Value<string>()!, Network.RegTest);
		Assert.NotEmpty(transaction.Inputs);
		Assert.All(transaction.Inputs, input => Assert.Contains(input.PrevOut, wallet.Coins.Select(c => c.Outpoint)));
		Assert.Contains(transaction.Outputs, output => output.ScriptPubKey == BitcoinAddress.Create(address, Network.RegTest).ScriptPubKey && output.Value == Money.Coins(0.005m));
		var verification = Network.RegTest.CreateTransactionBuilder().AddCoins(coins.Select(c => c.Coin));
		Assert.True(verification.Verify(transaction), "Automatic input selection must produce a valid signed transaction.");

		app.Global.FilterHeaders.SetServerTipHeight(new Height.ChainHeight(300));
		var scheme = new Scheme(app.Global);
		var details = JArray.Parse(scheme.ToJson(await scheme.ExecuteAsync("(unspent-coins (wallet))")));
		var detail = Assert.Single(details);
		Assert.Equal(coins[0].Transaction.GetConfirmations(300), detail["confirmations"]!.Value<uint>());
		Assert.Equal(2000000, detail["amount"]!.Value<long>());
		Assert.Equal("synthetic-funds", detail["labels"]!.Value<string>());
		Assert.DoesNotContain("exclude", details.ToString(), StringComparison.OrdinalIgnoreCase);
		await Assert.ThrowsAnyAsync<Exception>(() => scheme.ExecuteAsync("(coin-excluded-from-coinjoin? (car (wallet-unspent-coins (wallet))))"));
		coins[0].Transaction.SetUnconfirmed();
		var unconfirmed = JArray.Parse(scheme.ToJson(await scheme.ExecuteAsync("(unspent-coins (wallet))")));
		Assert.Equal(0, Assert.Single(unconfirmed)["confirmations"]!.Value<int>());
	}
}
