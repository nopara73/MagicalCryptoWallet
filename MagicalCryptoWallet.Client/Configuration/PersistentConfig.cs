using System;
using System.IO;
using NBitcoin;
using System.Net;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Userfacing;

namespace MagicalCryptoWallet.Client.Configuration;

public interface IPersistentConfig;

public record PersistentConfig(
	Network Network,
	string CoordinatorUri,
	string UseTor,
	bool TerminateTorOnExit,
	ValueList<string> TorBridges,
	bool DownloadNewVersion,
	bool JsonRpcServerEnabled,
	string JsonRpcUser,
	string JsonRpcPassword,
	ValueList<string> JsonRpcServerPrefixes,
	Money DustThreshold,
	bool EnableGpu,
	string CoordinatorIdentifier,
	string ExchangeRateProvider,
	string FeeRateEstimationProvider,
	string ExternalTransactionBroadcaster,
	decimal MaxCoinJoinMiningFeeRate,
	int AbsoluteMinInputCount,
	int MaxDaysInMempool,
	ValueList<string> ExperimentalFeatures,
	int ConfigVersion,
	bool UseTorForPublicData = false
	) : IPersistentConfig
{
	public string GetConfigFileName() =>
		Network switch
		{
			_ when Network == Network.Main => "Config.json",
			_ when Network == Network.TestNet => "Config.TestNet.json",
			_ when Network == Network.RegTest => "Config.RegTest.json",
			_ when Network == Bitcoin.Instance.Signet => "Config.Signet.json",
			_ => throw new NotSupportedException("Unsupported network")
		};
}
