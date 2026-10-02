using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using Newtonsoft.Json.Linq;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Client.Rpc;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Rpc;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Wallets;
using RuntimeWallet = MagicalCryptoWallet.Wallets.Wallet;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

[Collection("Serial unit tests collection")]
public class CoinJoinPolicyTests
{
	[Fact]
	public void RetiredStrategySettingsHaveNoConfigurationApi()
	{
		Assert.False(typeof(RuntimeWallet).GetProperty(nameof(RuntimeWallet.AnonScoreTarget))!.CanWrite);
		foreach (var name in new[] { "AutoCoinJoin", "AnonScoreTarget", "NonPrivateCoinIsolation", "OnlyUsePrivateFundsForPayments" })
		{
			Assert.Null(typeof(KeyManager).GetProperty(name));
		}
		Assert.Null(typeof(RuntimeWallet).GetProperty("ConsolidationMode"));
		Assert.Null(typeof(Config).GetProperty("AbsoluteMinInputCount"));
		Assert.Null(typeof(PersistentConfig).GetProperty("AbsoluteMinInputCount"));
		Assert.Null(typeof(CoinJoinConfiguration).GetProperty("AllowSoloCoinjoining"));
		Assert.Null(typeof(CoinJoinConfiguration).GetProperty("AbsoluteMinInputCount"));
		Assert.Null(typeof(CoinJoinTracker).GetProperty("StopWhenAllMixed"));
		Assert.Null(typeof(CoinJoinSnapshot).GetProperty("StopWhenAllMixed"));
		Assert.Null(typeof(KeyManager).Assembly.GetType("MagicalCryptoWallet.CoinJoinProfiles.PrivacyProfiles"));
	}
	[Theory]
	[InlineData("[false]")]
	[InlineData("[true]")]
	[InlineData("[null,false,true]")]
	[InlineData("{\"stopWhenAllMixed\":true}")]
	[InlineData("{\"overridePlebStop\":true}")]
	[InlineData("{\"password\":false}")]
	public async Task RemovedRpcParametersAreInvalidAsync(string parameters)
	{
		await using var app = new SingleWalletTests.SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var handler = new JsonRpcRequestHandler<MagicalCryptoWalletJsonRpcService>(new(app.Global), Network.RegTest);
		var response = JObject.Parse(await handler.HandleAsync("/", $$"""{"jsonrpc":"2.0","id":1,"method":"startcoinjoin","params":{{parameters}}}""", CancellationToken.None));
		Assert.Equal((int)JsonRpcErrorCodes.InvalidParams, response["error"]!["code"]!.Value<int>());
	}

	[Fact]
	public async Task WalletInfoReportsFixedPolicyAsync()
	{
		await using var app = new SingleWalletTests.SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		app.Session.Configure(app.NewKeys());
		var info = new MagicalCryptoWalletJsonRpcService(app.Global).WalletInfo();
		Assert.Equal(2, info["anonScoreTarget"]);
		Assert.Equal(true, info["isAutoCoinjoin"]);
		Assert.Equal(false, info["isNonPrivateCoinIsolation"]);
		Assert.Equal(false, info["onlyUsePrivateFundsForPayments"]);
	}

	[Theory]
	[InlineData("--absolutemininputcount=1")]
	[InlineData("--AbsoluteMinInputCount=21")]
	[InlineData("--absolutemininputcount")]
	public void RetiredMinimumInputOverridesAreRejected(string argument) =>
		Assert.Throws<ArgumentException>(() => new Config(PersistentConfigManager.DefaultRegTestConfig, [argument]));

	[Theory]
	[InlineData("1")]
	[InlineData("21")]
	[InlineData("")]
	public void RetiredMinimumInputEnvironmentOverrideIsRejected(string value)
	{
		const string key = "MAGICALCRYPTOWALLET_ABSOLUTEMININPUTCOUNT";
		var hadValue = Config.EnvironmentVariables.Contains(key);
		var previous = Config.EnvironmentVariables[key];
		try
		{
			Config.EnvironmentVariables[key] = value;
			Assert.Throws<ArgumentException>(() => new Config(PersistentConfigManager.DefaultRegTestConfig, []));
		}
		finally
		{
			if (hadValue) { Config.EnvironmentVariables[key] = previous; }
			else { Config.EnvironmentVariables.Remove(key); }
		}
	}

	[Fact]
	public async Task LegacyConfigOmitsRemovedMinimumButPreservesFeeCeilingAsync()
	{
		var path = Path.Combine(await Common.GetEmptyWorkDirAsync(), "Config.json");
		var expected = PersistentConfigManager.DefaultMainNetConfig with { MaxCoinJoinMiningFeeRate = 37m };
		var legacy = JObject.Parse(PersistentConfigManager.ToFile(path, expected));
		legacy["AbsoluteMinInputCount"] = 1;
		File.WriteAllText(path, legacy.ToString());
		var loaded = Assert.IsType<PersistentConfig>(PersistentConfigManager.LoadFile(path));
		Assert.Equal(expected, loaded);
		Assert.DoesNotContain("AbsoluteMinInputCount", PersistentConfigManager.ToFile(path, loaded));
	}

	[Fact]
	public async Task LegacyWalletKeepsRecoveryKeysLabelsAndBalanceSafeguardAsync()
	{
		var path = Path.Combine(await Common.GetEmptyWorkDirAsync(), "Wallet.json");
		var keys = KeyManager.CreateNew(out _, "synthetic-passphrase", Network.RegTest);
		keys.SetFilePath(path);
		keys.PlebStopThreshold = Money.Coins(0.008m);
		keys.GenerateNewKey("synthetic-label", KeyState.Used, false);
		keys.ToFile();
		var original = JObject.Parse(File.ReadAllText(path));
		var legacy = (JObject)original.DeepClone();
		foreach (var name in new[] { "AutoCoinJoin", "AnonScoreTarget", "RedCoinIsolation", "OnlyUsePrivateFundsForPayments", "ConsolidationMode", "StopWhenAllMixed" })
		{
			legacy[name] = 99; // Retired fields are ignored even when they have the old type wrong.
		}
		File.WriteAllText(path, legacy.ToString());
		var loaded = KeyManager.FromFile(path);
		Assert.Equal(Money.Coins(0.008m), loaded.PlebStopThreshold);
		Assert.Equal(keys.EncryptedSecret, loaded.EncryptedSecret);
		Assert.Equal(keys.ChainCode, loaded.ChainCode);
		Assert.Equal(keys.SegwitExtPubKey, loaded.SegwitExtPubKey);
		Assert.Equal(keys.TaprootExtPubKey, loaded.TaprootExtPubKey);
		Assert.Contains(loaded.GetKeys(), key => key.Labels.Contains("synthetic-label"));
		loaded.ToFile();
		var saved = JObject.Parse(File.ReadAllText(path));
		foreach (var name in original.Properties().Select(p => p.Name))
		{
			if (name == "HdPubKeys")
			{
				Assert.All(original[name]!, key => Assert.Contains(saved[name]!, savedKey => JToken.DeepEquals(key, savedKey)));
			}
			else { Assert.True(JToken.DeepEquals(original[name], saved[name]), $"Retained wallet field changed: {name}"); }
		}
		foreach (var name in legacy.Properties().Select(p => p.Name).Except(original.Properties().Select(p => p.Name)))
		{
			Assert.False(saved.ContainsKey(name));
		}
		Assert.Equal(Constants.DefaultMaxCoinJoinMiningFeeRate, 50m);
	}
}
