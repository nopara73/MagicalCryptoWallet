using System.IO;
using System.Linq;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Serialization;
using MagicalCryptoWallet.Tests.Helpers;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Client.Configuration;

/// <summary>
/// Tests for <see cref="PersistentConfigManager"/>
/// </summary>
public class PersistentConfigManagerTests
{
	[Fact]
	public async Task ToFileAndLoadFileTestAsync()
	{
		string workDirectory = await Common.GetEmptyWorkDirAsync();
		string configPath = Path.Combine(workDirectory, $"{nameof(ToFileAndLoadFileTestAsync)}.json");

		// Create config and store it.
		PersistentConfig actualConfig = PersistentConfigManager.DefaultMainNetConfig;

		string storedJson = PersistentConfigManager.ToFile(configPath, actualConfig);
		PersistentConfigManager.UpdateNetwork(configPath, actualConfig.Network);

		var readConfig = PersistentConfigManager.LoadFile(configPath) as PersistentConfig;
		Assert.NotNull(readConfig);

		// Objects are supposed to be equal by value-equality rules.
		Assert.Equal(actualConfig, readConfig);

		// Check that JSON strings are equal as well.
		{
			// JsonRpcUser and JsonRpcPassword are randomly generated, so we use the actual values
			string expected = GetConfigString(actualConfig.JsonRpcUser, actualConfig.JsonRpcPassword);
			string actual = JsonEncoder.ToReadableString(readConfig, PersistentConfigEncode.PersistentConfig);

			AssertJsonStringsEqual(expected, actual);
			AssertJsonStringsEqual(expected, storedJson);
		}

		static string GetConfigString(string jsonRpcUser, string jsonRpcPassword)
			=> $$"""
			{
			  "CoordinatorUri": "",
			  "UseTor": "Enabled",
			  "UseTorForPublicData": false,
			  "TerminateTorOnExit": false,
			  "TorBridges": [],
			  "DownloadNewVersion": true,
			  "JsonRpcServerEnabled": false,
			  "JsonRpcUser": "{{jsonRpcUser}}",
			  "JsonRpcPassword": "{{jsonRpcPassword}}",
			  "JsonRpcServerPrefixes": [
			    "http://127.0.0.1:38128/",
			    "http://localhost:38128/"
			  ],
			  "DustThreshold": "0.00001",
			  "EnableGpu": true,
			  "CoordinatorIdentifier": "CoinJoinCoordinatorIdentifier",
			  "ExchangeRateProvider": "MempoolSpace",
			  "FeeRateEstimationProvider": "MempoolSpace",
			  "ExternalTransactionBroadcaster": "MempoolSpace",
			  "MaxCoinJoinMiningFeeRate": 50.0,
			  "MaxDaysInMempool": 30,
			  "ExperimentalFeatures": [],
			  "ConfigVersion": 4
			}
			""";

		static void AssertJsonStringsEqual(string expected, string actual)
			=> Assert.Equal(expected.ReplaceLineEndings("\n"), actual.ReplaceLineEndings("\n"));
	}

	[Theory]
	[InlineData("Config.json")]
	[InlineData("Config.TestNet.json")]
	[InlineData("Config.Signet.json")]
	[InlineData("Config.RegTest.json")]
	public async Task VersionFourCoreFieldsAreIgnoredAndNotSavedAsync(string fileName)
	{
		var expected = PersistentConfigManager.DefaultMainNetConfig with
		{
			CoordinatorUri = "http://coordinator.invalid/",
			UseTor = "Disabled",
			DustThreshold = Money.Coins(0.00002m),
			FeeRateEstimationProvider = "None",
			JsonRpcServerEnabled = true,
			JsonRpcUser = "synthetic-user",
			JsonRpcPassword = "synthetic-password"
		};
		var path = Path.Combine(await Common.GetEmptyWorkDirAsync(), fileName);
		var legacy = JsonNode.Parse(JsonEncoder.ToReadableString(expected, PersistentConfigEncode.PersistentConfig))!;
		legacy["BitcoinRpcEndPoint"] = "invalid endpoint that must never be parsed";
		legacy["BitcoinRpcUri"] = new JsonArray(1, 2);
		legacy["BitcoinRpcCredentialString"] = new JsonObject { ["obsolete"] = "synthetic credential" };
		File.WriteAllText(path, legacy.ToJsonString());

		var loaded = Assert.IsType<PersistentConfig>(PersistentConfigManager.LoadFile(path));
		Assert.Equal(expected, loaded);
		var saved = PersistentConfigManager.ToFile(path, loaded);
		Assert.DoesNotContain("BitcoinRpc", saved);
		Assert.Equal(4, JsonNode.Parse(saved)!["ConfigVersion"]!.GetValue<int>());
		Assert.Equal(expected, PersistentConfigManager.LoadFile(path));
	}

	[Fact]
	public void RetiredCoreArgumentsCannotOverrideClientConfiguration()
	{
		var config = new Config(PersistentConfigManager.DefaultMainNetConfig,
			["--bitcoinrpcendpoint=http://127.0.0.1:8332", "--bitcoinrpccredentialstring=synthetic:synthetic"]);
		Assert.False(config.IsOverridden);
		Assert.Equal(Network.Main, config.Network);
		Assert.Equal(PersistentConfigManager.DefaultMainNetConfig.FeeRateEstimationProvider, config.FeeRateEstimationProvider);
	}

	[Fact]
	public void RetiredCoreEnvironmentKeysCannotOverrideClientConfiguration()
	{
		var keys = new[] { "MAGICALCRYPTOWALLET_BITCOINRPCENDPOINT", "MAGICALCRYPTOWALLET_BITCOINRPCURI", "MAGICALCRYPTOWALLET_BITCOINRPCCREDENTIALSTRING" };
		var original = keys.Select(key => (Key: key, Exists: Config.EnvironmentVariables.Contains(key), Value: Config.EnvironmentVariables[key])).ToArray();
		try
		{
			foreach (var key in keys) { Config.EnvironmentVariables[key] = "obsolete synthetic value"; }
			var config = new Config(PersistentConfigManager.DefaultMainNetConfig, []);
			Assert.False(config.IsOverridden);
			Assert.Equal(Network.Main, config.Network);
			Assert.Equal(PersistentConfigManager.DefaultMainNetConfig.FeeRateEstimationProvider, config.FeeRateEstimationProvider);
		}
		finally
		{
			foreach (var (key, exists, value) in original)
			{
				if (exists) { Config.EnvironmentVariables[key] = value; }
				else { Config.EnvironmentVariables.Remove(key); }
			}
		}
	}
}
