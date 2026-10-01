using NBitcoin;
using Newtonsoft.Json;
using MagicalCryptoWallet.Blockchain.Transactions;

namespace MagicalCryptoWallet.Rpc.JsonConverters;

public class SmartTransactionJsonConverter : JsonConverter<SmartTransaction>
{
	public override SmartTransaction? ReadJson(JsonReader reader, Type objectType, SmartTransaction? existingValue, bool hasExistingValue, JsonSerializer serializer)
	{
		throw new NotImplementedException();
	}

	/// <inheritdoc />
	public override void WriteJson(JsonWriter writer, SmartTransaction? value, JsonSerializer serializer)
	{
		serializer.Serialize(writer, value?.Transaction);
	}
}
