using NBitcoin;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Serialization;
using MagicalCryptoWallet.WabiSabi.Coordinator;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Client.Configuration;

public class TorPublicDataConfigTests
{
	[Theory]
	[InlineData("Main", "Enabled", false, true)]
	[InlineData("Main", "EnabledOnlyRunning", false, true)]
	[InlineData("Main", "Enabled", true, false)]
	[InlineData("Main", "Disabled", false, false)]
	[InlineData("Main", "Disabled", true, false)]
	[InlineData("RegTest", "Enabled", false, false)]
	[InlineData("RegTest", "Enabled", true, false)]
	public void PeerPoolsSplitOnlyWhenWalletTrafficUsesTor(string network, string torMode, bool publicDataTor, bool split)
	{
		var config = new Config(PersistentConfigManager.DefaultMainNetConfig with
		{
			Network = Network.GetNetwork(network)!, UseTor = torMode, UseTorForPublicData = publicDataTor
		}, []);
		Assert.Equal(split && !PlatformInformation.IsTailsOS() && !PlatformInformation.IsWhonix(), config.UseSeparatePublicPeerPool);
	}

	[Theory]
	[InlineData(false)]
	[InlineData(true)]
	public void PublicDataSettingRoundTrips(bool useTor)
	{
		var original = PersistentConfigManager.DefaultMainNetConfig with { UseTorForPublicData = useTor };
		var json = JsonEncoder.ToReadableString(original, PersistentConfigEncode.PersistentConfig);
		var decoded = JsonDecoder.FromString(json, PersistentConfigDecode.CurrentPersistentConfig);
		Assert.Equal(original, decoded);
	}

	[Fact]
	public void OlderClientConfigUsesDirectPublicData()
	{
		var json = PersistentConfigEncode.PersistentConfig(PersistentConfigManager.DefaultMainNetConfig).AsObject();
		json.Remove("UseTorForPublicData");
		var config = JsonDecoder.FromString(json.ToJsonString(), PersistentConfigDecode.CurrentPersistentConfig);
		Assert.NotNull(config);
		Assert.False(config.UseTorForPublicData);
	}

	[Theory]
	[InlineData(true, "--usetorforpublicdata=false", false)]
	[InlineData(false, "--usetorforpublicdata=true", true)]
	public void CommandLineOverridesPublicDataRouting(bool persisted, string argument, bool expected)
	{
		var config = new Config(PersistentConfigManager.DefaultMainNetConfig with { UseTorForPublicData = persisted }, [argument]);
		Assert.Equal(expected || PlatformInformation.IsTailsOS() || PlatformInformation.IsWhonix(), config.UseTorForPublicData);
	}

	[Theory]
	[InlineData(true)]
	[InlineData(false)]
	public void OlderCoordinatorConfigPreservesItsOutboundPolicy(bool publishesOnion)
	{
		var json = Encode.WabiSabiConfig(new WabiSabiConfig("synthetic-config.json") { PublishAsOnionService = publishesOnion }).AsObject();
		json.Remove("UseTorForPublicData");
		var decoded = JsonDecoder.FromString(json.ToJsonString(), Decode.WabiSabiConfig("synthetic-config.json"));
		Assert.NotNull(decoded);
		Assert.Equal(publishesOnion, decoded.UseTorForPublicData);
	}

	[Theory]
	[InlineData(true, false)]
	[InlineData(false, true)]
	public void CoordinatorOutboundRoutingIsIndependentOfPublishingOnion(bool publishesOnion, bool publicDataTor)
	{
		var config = new WabiSabiConfig("synthetic-config.json") { PublishAsOnionService = publishesOnion, UseTorForPublicData = publicDataTor };
		var decoded = JsonDecoder.FromString(Encode.WabiSabiConfig(config).ToJsonString(), Decode.WabiSabiConfig("synthetic-config.json"));
		Assert.NotNull(decoded);
		Assert.Equal(publishesOnion, decoded.PublishAsOnionService);
		Assert.Equal(publicDataTor, decoded.UseTorForPublicData);
	}
}
