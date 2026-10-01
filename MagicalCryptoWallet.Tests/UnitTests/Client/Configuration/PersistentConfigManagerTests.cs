using System.IO;
using System.Linq;
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
			  "TerminateTorOnExit": false,
			  "TorBridges": [],
			  "DownloadNewVersion": true,
			  "BitcoinRpcCredentialString": "",
			  "BitcoinRpcEndPoint": "",
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
			  "AbsoluteMinInputCount": 21,
			  "MaxDaysInMempool": 30,
			  "ExperimentalFeatures": [],
			  "ConfigVersion": 4
			}
			""";

		static void AssertJsonStringsEqual(string expected, string actual)
			=> Assert.Equal(expected.ReplaceLineEndings("\n"), actual.ReplaceLineEndings("\n"));
	}

	// Test for migration 2.6.0 -> 2.8.0
}
