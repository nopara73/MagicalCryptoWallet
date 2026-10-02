using System.Reflection;
using NBitcoin;

namespace MagicalCryptoWallet.Helpers;

public static class Constants
{
	public const string CoordinatorUri = "";
	public const string TestnetCoordinatorUri = "";
	public const string SignetCoordinatorUri = "";
	public const string RegTestCoordinatorUri = "http://localhost:38126/";

	public const string UpdateSignaturePublicKey = "02a85e26e9e8dd0b6d06d166b1f1c427a06e09e4055f016fc55200cb9ba2dc5ef9";


	public const string ReleaseAnnouncementNpub = "npub10sjxe0qhl73rqkgwkxhl46ya7el8s4s5ts79m5g7z8luq49927tscjwzeg";
	public const string ApplicationId = "io.github.nopara73.magicalcryptowallet";
	public const string RepositoryUrl = "https://github.com/nopara73/MagicalCryptoWallet";

	/// <summary>
	/// By changing this, we can force to start over the transactions file, so old incorrect transactions would be cleared.
	/// It is also important to force the KeyManagers to be reindexed when this is changed by renaming the BlockState Height related property.
	/// </summary>
	public const string ConfirmedTransactionsVersion = "2";

	public const int ResyncHeightMargin = 101;
	public const uint ProtocolVersionWitnessVersion = 70012;

	public const int InputBaseSizeInBytes = 41;

	public const int P2wpkhInputSizeInBytes = 41;
	public const int P2wpkhInputVirtualSize = 69;
	public const int P2pkhInputSizeInBytes = 145;
	public const int P2wpkhOutputVirtualSize = 31;

	public const int P2trInputVirtualSize = 58;
	public const int P2trOutputVirtualSize = 43;

	public const int P2pkhInputVirtualSize = 148;
	public const int P2pkhOutputVirtualSize = 34;
	public const int P2wshInputVirtualSize = 105; // we assume a 2-of-n multisig
	public const int P2wshOutputVirtualSize = 32;
	public const int P2shInputVirtualSize = 297; // we assume a 2-of-n multisig
	public const int P2shOutputVirtualSize = 32;

	// https://en.bitcoin.it/wiki/Bitcoin
	// There are a maximum of 2,099,999,997,690,000 Bitcoin elements (called satoshis), which are currently most commonly measured in units of 100,000,000 known as BTC. Stated another way, no more than 21 million BTC can ever be created.
	public const long MaximumNumberOfSatoshis = 2099999997690000;

	public const decimal MaximumNumberOfBitcoins = 20999999.9769m;

	public const int AnonymityScoreTarget = 2;
	public const int CoinJoinMinimumInputCount = 21;

	public const int SemiPrivateThreshold = AnonymityScoreTarget;

	public const int FastestConfirmationTarget = 1;
	public const int TwentyMinutesConfirmationTarget = 2;
	public const int OneDayConfirmationTarget = 144;
	public const int SevenDaysConfirmationTarget = 1008;

	public const int DefaultMainNetBitcoinRpcPort = 8332;
	public const int DefaultTestNetBitcoinRpcPort = 48332;
	public const int DefaultSignetBitcoinRpcPort = 38332;
	public const int DefaultRegTestBitcoinCorePort = 18443;

	public const decimal DefaultDustThreshold = 0.00001m;
	public const decimal DefaultMaxCoinJoinMiningFeeRate = 50.0m;
	public const int AbsoluteMinInputCount = 2;

	public const string AlphaNumericCharacters = "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
	public const string CapitalAlphaNumericCharacters = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";


	/// <summary>Executable file name of Magical Crypto Wallet UI application (without extension).</summary>
	public const string ExecutableName = "magicalcryptowallet";

	/// <summary>Plist name, only for MacOs. Starts MagicalCryptoWallet with -startsilent argument.</summary>
	public const string SilentPlistName = ApplicationId + ".startup.plist";

	public const string AppName = "Magical Crypto Wallet";

	public static readonly string DefaultMainNetBitcoinRpcUri = "";
	public static readonly string DefaultTestNetBitcoinRpcUri = $"http://localhost:{DefaultTestNetBitcoinRpcPort}";
	public static readonly string DefaultSignetBitcoinRpcUri = $"http://localhost:{DefaultSignetBitcoinRpcPort}";
	public static readonly string DefaultRegTestBitcoinRpcUri = $"http://localhost:{DefaultRegTestBitcoinCorePort}";

	public const string DefaultExchangeRateProvider = "MempoolSpace";
	public const string DefaultFeeRateEstimationProvider = "MempoolSpace";
	public const string DefaultExternalTransactionBroadcaster= "MempoolSpace";

	public static readonly Money MaximumNumberOfBitcoinsMoney = Money.Coins(MaximumNumberOfBitcoins);

	public static readonly Version ClientVersion = Version.Parse(
		typeof(Constants).Assembly.GetCustomAttributes<System.Reflection.AssemblyMetadataAttribute>()
		.Single(attribute => attribute.Key == "ClientVersion").Value!);
	public static readonly string VersionName = "";

	public static readonly FeeRate MinRelayFeeRate = new(0.1m);
	public static readonly FeeRate AbsurdlyHighFeeRate = new(10_000m);

	public const decimal BnBMaximumDifferenceTolerance = 0.15m;
	public const int DefaultMaxDaysInMempool = 30;

	// Defined in hours. Do not modify these values or the order!
	public static readonly int[] CoinJoinFeeRateMedianTimeFrames = new[] { 24, 168, 720 };

	public static readonly string[] UserAgents = new[]
	{
		"/Satoshi:30.2.0/",
		"/Satoshi:30.0.0/",
		"/Satoshi:29.3.0/",
		"/Satoshi:29.2.0/",
		"/Satoshi:29.0.0/",
		"/Satoshi:28.1.0/",
		"/Satoshi:28.0.0/",
		"/Satoshi:27.2.0/",
		"/Satoshi:27.1.0/",
		"/Satoshi:27.0.0/",
		"/Satoshi:26.2.0/",
		"/Satoshi:26.1.0/",
		"/Satoshi:26.0.0/",
	};

	public static readonly int[] ConfirmationTargets = new[]
	{
		2, // Twenty Minutes
		3, // Thirty Minutes
		6, // One Hour
		18, // Three Hours
		36, // Six Hours
		72, // Twelve Hours
		144, // One Day
		432, // Three Days
		1008, // Seven Days
	};

	public static readonly string DefaultFilterType = "legacy";
}
