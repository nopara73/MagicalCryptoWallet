using System.Text.Json.Nodes;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Serialization;
using static MagicalCryptoWallet.Serialization.Encode;
using static MagicalCryptoWallet.Serialization.Decode;

namespace MagicalCryptoWallet.Backend;

public static class ConfigEncode
{
	public static JsonNode Config(Config cfg) =>
		Object([
			("Network", Network(cfg.Network) ),
			("BitcoinRpcConnectionString", String(cfg.BitcoinRpcConnectionString) ),
			("MainNetBitcoinCoreRpcEndPoint", String(cfg.MainNetBitcoinRpcUri) ),
			("TestNetBitcoinCoreRpcEndPoint", String(cfg.TestNetBitcoinRpcUri) ),
			("RegTestBitcoinCoreRpcEndPoint", String(cfg.RegTestBitcoinRpcUri) ),
			("SignetBitcoinCoreRpcEndPoint", String(cfg.SignetBitcoinRpcUri) ),
			("FilterType", Constants.DefaultFilterType)
		]);
}

public static class ConfigDecode
{
	public static Decoder<Config> Config(string filePath) =>
		Object(get => new Config(
			filePath,
			get.Required("Network", Decode.Network ),
			get.Required("BitcoinRpcConnectionString", Decode.String ),
			get.Required("MainNetBitcoinCoreRpcEndPoint", Decode.String ),
			get.Required("TestNetBitcoinCoreRpcEndPoint", Decode.String ),
			get.Required("RegTestBitcoinCoreRpcEndPoint", Decode.String ),
			get.Required("SignetBitcoinCoreRpcEndPoint", Decode.String),
			get.Optional("FilterType", Decode.String) ?? Constants.DefaultFilterType
		));
}
