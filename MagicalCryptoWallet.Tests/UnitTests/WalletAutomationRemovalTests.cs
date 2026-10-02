using System;
using System.IO;
using System.Linq;
using System.Text.Json.Nodes;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Serialization;
using MagicalCryptoWallet.Tests.Helpers;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

[Collection("Serial unit tests collection")]
public class WalletAutomationRemovalTests
{
	private static readonly string[] RetiredSettings =
		["ExperimentalFeatures", "JsonRpcServerEnabled", "JsonRpcUser", "JsonRpcPassword", "JsonRpcServerPrefixes", "RpcOnionEnabled"];

	[Theory]
	[InlineData("Config.json", false)]
	[InlineData("Config.TestNet.json", false)]
	[InlineData("Config.Signet.json", false)]
	[InlineData("Config.RegTest.json", false)]
	[InlineData("Config.json", true)]
	[InlineData("Config.TestNet.json", true)]
	[InlineData("Config.Signet.json", true)]
	[InlineData("Config.RegTest.json", true)]
	public async Task LegacyAutomationSettingsAreIgnoredWithoutResettingPreferencesAsync(string name, bool malformed)
	{
		var baseline = name switch
		{
			"Config.TestNet.json" => PersistentConfigManager.DefaultTestNetConfig,
			"Config.Signet.json" => PersistentConfigManager.DefaultSignetConfig,
			"Config.RegTest.json" => PersistentConfigManager.DefaultRegTestConfig,
			_ => PersistentConfigManager.DefaultMainNetConfig
		};
		var expected = baseline with
		{
			// Version 4 stores the selected network in the separate network marker.
			Network = Network.Main,
			CoordinatorUri = "http://coordinator.invalid/",
			UseTor = "Disabled",
			DustThreshold = Money.Coins(0.00002m),
			FeeRateEstimationProvider = "None",
			MaxCoinJoinMiningFeeRate = 37m
		};
		var legacy = JsonNode.Parse(JsonEncoder.ToReadableString(expected, PersistentConfigEncode.PersistentConfig))!;
		legacy["ExperimentalFeatures"] = malformed ? new JsonObject { ["obsolete"] = true } : new JsonArray("scripting");
		legacy["JsonRpcServerEnabled"] = malformed ? new JsonArray(1) : JsonValue.Create(true);
		legacy["JsonRpcUser"] = malformed ? new JsonArray(2) : JsonValue.Create("synthetic-user");
		legacy["JsonRpcPassword"] = malformed ? new JsonObject { ["obsolete"] = true } : JsonValue.Create("synthetic-password");
		legacy["JsonRpcServerPrefixes"] = malformed ? JsonValue.Create(123) : new JsonArray("http://127.0.0.1:38128/");
		legacy["RpcOnionEnabled"] = malformed ? new JsonObject { ["obsolete"] = true } : JsonValue.Create(true);
		var path = Path.Combine(await Common.GetEmptyWorkDirAsync(), name);
		PersistentConfigManager.UpdateNetwork(path, baseline.Network);
		await File.WriteAllTextAsync(path, legacy.ToJsonString());

		var loaded = Assert.IsType<PersistentConfig>(PersistentConfigManager.LoadFile(path));
		Assert.Equal(expected, loaded);
		var saved = JsonNode.Parse(PersistentConfigManager.ToFile(path, loaded))!;
		Assert.Equal(4, saved["ConfigVersion"]!.GetValue<int>());
		Assert.All(RetiredSettings, key => Assert.Null(saved[key]));
		Assert.Equal(expected, PersistentConfigManager.LoadFile(path));
		Assert.Equal(baseline.Network.ToString(), await File.ReadAllTextAsync(Path.Combine(Path.GetDirectoryName(path)!, "network")));
	}

	[Fact]
	public void RetiredOverridesCannotReenableAutomationOrMarkSettingsOverridden()
	{
		var original = RetiredSettings.Select(key =>
		{
			var environmentKey = "MAGICALCRYPTOWALLET_" + key.ToUpperInvariant();
			return (Key: environmentKey, Exists: Config.EnvironmentVariables.Contains(environmentKey), Value: Config.EnvironmentVariables[environmentKey]);
		}).ToArray();
		try
		{
			foreach (var entry in original) { Config.EnvironmentVariables[entry.Key] = "scripting"; }
			var config = new Config(PersistentConfigManager.DefaultRegTestConfig,
				RetiredSettings.Select(key => "--" + key.ToLowerInvariant() + "=scripting").ToArray());
			Assert.False(config.IsOverridden);
			Assert.Equal(Network.RegTest, config.Network);
			Assert.DoesNotContain(Config.GetConfigOptionsMetadata(), option => RetiredSettings.Contains(option.ParameterName, StringComparer.OrdinalIgnoreCase));
		}
		finally
		{
			foreach (var entry in original)
			{
				if (entry.Exists) { Config.EnvironmentVariables[entry.Key] = entry.Value; }
				else { Config.EnvironmentVariables.Remove(entry.Key); }
			}
		}
	}

	[Fact]
	public void RetiredRuntimeInterfacesAreAbsent()
	{
		Assert.Null(typeof(Global).Assembly.GetType("MagicalCryptoWallet.Client.Scheme"));
		Assert.Null(typeof(Global).Assembly.GetType("MagicalCryptoWallet.Client.MagicalCryptoWalletLibGenerator"));
		Assert.Null(typeof(Global).Assembly.GetType("MagicalCryptoWallet.Client.Rpc.MagicalCryptoWalletJsonRpcService"));
		Assert.Null(typeof(KeyManager).Assembly.GetType("MagicalCryptoWallet.Rpc.JsonRpcServer"));
		Assert.Null(typeof(App).Assembly.GetType("MagicalCryptoWallet.Fluent.ViewModels.Scheme.SchemeConsoleViewModel"));
		Assert.Null(typeof(UiContext).GetProperty("Scheme"));
		Assert.Null(typeof(Global).GetProperty("RpcServer"));
		Assert.All(RetiredSettings, key => Assert.Null(typeof(Config).GetProperty(key)));
		Assert.All(RetiredSettings, key => Assert.Null(typeof(PersistentConfig).GetProperty(key)));
	}
}
