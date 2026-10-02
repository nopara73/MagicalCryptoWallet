using NBitcoin;
using Newtonsoft.Json;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;

namespace MagicalCryptoWallet.Rpc.JsonConverters;

public class DestinationJsonConverter(Network network) : JsonConverter<Destination>
{
	public override void WriteJson(JsonWriter writer, Destination? value, JsonSerializer serializer) =>
		writer.WriteValue(value?.ScriptPubKey.GetDestinationAddress(network)?.ToString());

	public override Destination? ReadJson(JsonReader reader, Type objectType, Destination? existingValue, bool hasExistingValue,
		JsonSerializer serializer)
	{
		var address = reader.Value as string;
		ArgumentException.ThrowIfNullOrWhiteSpace(address);
		return BitcoinAddress.Create(address, network);
	}
}
