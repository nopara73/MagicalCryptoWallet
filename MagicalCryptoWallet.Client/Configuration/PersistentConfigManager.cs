using System;
using System.IO;
using System.Text;
using NBitcoin;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Serialization;

namespace MagicalCryptoWallet.Client.Configuration;

public static class PersistentConfigManager
{
	private static readonly RandomStringGenerator GenerateRandomString = RandomnessProviders.Secure.CreateRandomStringGenerator();

	public static readonly PersistentConfig DefaultMainNetConfig = new (
		Network : Network.Main,
		CoordinatorUri : Constants.CoordinatorUri,
		UseTor : GetDefaultTorMode(),
		TerminateTorOnExit : false,
		TorBridges : [],
		DownloadNewVersion : true,
		JsonRpcServerEnabled : false,
		JsonRpcUser : GenerateRandomString(12),
		JsonRpcPassword : GenerateRandomString(12),
		JsonRpcServerPrefixes : new (["http://127.0.0.1:38128/", "http://localhost:38128/"]),
		DustThreshold : Money.Coins(Constants.DefaultDustThreshold),
		EnableGpu : true,
		CoordinatorIdentifier : "CoinJoinCoordinatorIdentifier",
		ExchangeRateProvider : Constants.DefaultExchangeRateProvider,
		FeeRateEstimationProvider : Constants.DefaultFeeRateEstimationProvider,
		ExternalTransactionBroadcaster : Constants.DefaultExternalTransactionBroadcaster,
		MaxCoinJoinMiningFeeRate : Constants.DefaultMaxCoinJoinMiningFeeRate,
		MaxDaysInMempool : Constants.DefaultMaxDaysInMempool,
		ExperimentalFeatures: [],
		ConfigVersion : 4);

	public static readonly PersistentConfig DefaultTestNetConfig = DefaultMainNetConfig with
	{
		Network = Network.TestNet,
		CoordinatorUri = Constants.TestnetCoordinatorUri,
		JsonRpcServerEnabled = true,
		ExperimentalFeatures = new ValueList<string>(["scripting"]),
	};

	public static readonly PersistentConfig DefaultRegTestConfig = DefaultTestNetConfig with
	{
		Network = Network.RegTest,
		CoordinatorUri = Constants.RegTestCoordinatorUri,
	};

	public static readonly PersistentConfig DefaultSignetConfig = DefaultTestNetConfig with
	{
		Network = Bitcoin.Instance.Signet,
		CoordinatorUri = Constants.SignetCoordinatorUri,
	};

	public static string ToFile(string filePath, PersistentConfig obj)
	{
		string jsonString = JsonEncoder.ToReadableString(obj, PersistentConfigEncode.PersistentConfig);
		File.WriteAllText(filePath, jsonString, Encoding.UTF8);

		return jsonString;
	}

	public static void UpdateNetwork(string filePath, Network network)
	{
		var networkFilePath = Path.Combine(Path.GetDirectoryName(filePath) ?? string.Empty, "network");
		File.WriteAllText(networkFilePath, network.ToString());
	}

	public static IPersistentConfig LoadFile(string filePath)
	{
		try
		{
			using var cfgFile = File.Open(filePath, FileMode.Open, FileAccess.Read);
			var decoder = JsonDecoder.FromStream(PersistentConfigDecode.PersistentConfig);
			var decodingResult = decoder(cfgFile);
			return decodingResult.Match(cfg => cfg, error => throw new InvalidOperationException(error));
		}
		catch (FileNotFoundException)
		{
			var defaultConfig = GetDefaultPersistentConfigByFileName(filePath);

			ToFile(filePath, defaultConfig);
			UpdateNetwork(filePath, defaultConfig.Network);

			Logger.LogInfo($"File did not exist. Created at path: '{filePath}'.");
			return defaultConfig;
		}
		catch (Exception ex)
		{
			var defaultConfig = GetDefaultPersistentConfigByFileName(filePath);

			ToFile(filePath, defaultConfig);
			UpdateNetwork(filePath, defaultConfig.Network);

			Logger.LogInfo($"{nameof(Config)} file has been deleted because it was corrupted. Recreated default version at path: '{filePath}'.");
			Logger.LogWarning(ex);
			return defaultConfig;
		}

		static PersistentConfig GetDefaultPersistentConfigByFileName(string configFilePath) =>
			Path.GetFileName(configFilePath) switch
			{
				"Config.json" => DefaultMainNetConfig,
				"Config.TestNet.json" => DefaultTestNetConfig,
				"Config.RegTest.json" => DefaultRegTestConfig,
				"Config.Signet.json" => DefaultSignetConfig,
				_ => throw new ArgumentException($"The file '{configFilePath}' is not a valid config file name.")
			};
	}

	private static string GetDefaultTorMode()
	{
		// On Tails and Whonix, Tor is already running system-wide
		// We should only connect to it, not start our own instance
		if (PlatformInformation.IsTailsOS())
		{
			Logger.LogInfo("Detected Tails operating system. Setting Tor mode to 'Connect Only' by default.");
			return "EnabledOnlyRunning";
		}
		else if (PlatformInformation.IsWhonix())
		{
			Logger.LogInfo("Detected Whonix operating system. Setting Tor mode to 'Connect Only' by default.");
			return "EnabledOnlyRunning";
		}

		return "Enabled";
	}
}
