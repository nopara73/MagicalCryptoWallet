using NBitcoin;
using Newtonsoft.Json;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Rpc.JsonConverters;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.JsonConverters;

public class DestinationJsonConverterTests
{
	[Fact]
	public void StandardBitcoinDestinationRoundTrips()
	{
		const string address = "18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX";
		var converter = new DestinationJsonConverter(Network.Main);
		var json = JsonConvert.SerializeObject(address);
		var destination = JsonConvert.DeserializeObject<Destination>(json, converter);
		Assert.NotNull(destination);
		Assert.Equal(BitcoinAddress.Create(address, Network.Main).ScriptPubKey, destination.ScriptPubKey);
		Assert.Equal(json, JsonConvert.SerializeObject(destination, converter));
	}

	[Theory]
	[InlineData("sp1qqgste7k9hx0qftg6qmwlkqtwuy6cycyavzmzj85c6qdfhjdpdjtdgqjuexzk6murw56suy3e0rd2cgqvycxttddwsvgxe2usfpxumr70xc9pkqwv")]
	[InlineData("mipcBbFg9gMiCh81Kj8tqqdgoZub1ZJRfn")]
	[InlineData("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX")]
	public void RejectsUnsupportedAndWrongNetworkDestinations(string address)
	{
		Assert.Throws<FormatException>(() => JsonConvert.DeserializeObject<Destination>(
			JsonConvert.SerializeObject(address), new DestinationJsonConverter(Network.Main)));
	}
}
