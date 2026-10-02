using System.Collections.Generic;
using System.Reflection;
using System.Threading.Tasks;
using NBitcoin.Protocol.Behaviors;
using MagicalCryptoWallet.BitcoinP2p;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Serialization;
using MagicalCryptoWallet.Services.NodesManagement;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.WabiSabi.Coordinator;
using MagicalCryptoWallet.WebClients.MagicalCryptoWallet;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Client.Configuration;

public class PublicNetworkRoutingTests
{
	[Fact]
	public async Task PublicPeersReceiveDirectlyAndBroadcastPeersAreProtectedAsync()
	{
		var config = new Config(PersistentConfigManager.DefaultMainNetConfig with { UseTor = "Enabled", CoordinatorUri = "" }, []);
		var global = new Global(await Common.GetEmptyWorkDirAsync(), config);
		try
		{
			Assert.IsType<DirectHttpClientFactory>(global.PublicSourcesHttpClientFactory);
			var publicPool = Field<P2pConnectionManager>(global, "_publicConnectionManager");
			var broadcastPool = Field<P2pConnectionManager>(global, "_p2pConnectionManager");
			Assert.Null(Field<object?>(publicPool, "_torSocks5"));
			var options = Field<P2pConnectionOptions>(publicPool, "_options");
			Assert.True(options.AllowBlockDownloads);
			Assert.False(options.AllowTransactionBroadcasts);
			var behavior = Assert.IsType<P2pBehavior>(Assert.Single(Field<List<NodeBehavior>>(publicPool, "_templateBehaviors")));
			Assert.True(behavior.ListenForTransactions);
			Assert.False(behavior.ServeBroadcasts);
			Assert.NotSame(publicPool, broadcastPool);
			Assert.NotNull(Field<object?>(broadcastPool, "_torSocks5"));
			var protectedOptions = Field<P2pConnectionOptions>(broadcastPool, "_options");
			Assert.False(protectedOptions.AllowBlockDownloads);
			Assert.Equal(4, protectedOptions.TargetConnections);
			Assert.True(protectedOptions.RelayTransactions);
			Assert.Equal(0, protectedOptions.MinimumCompactFilterNodes);
			Assert.Equal(0, Field<int>(broadcastPool, "_started"));
			var protectedBehavior = Assert.IsType<P2pBehavior>(Assert.Single(Field<List<NodeBehavior>>(broadcastPool, "_templateBehaviors")));
			Assert.False(protectedBehavior.ListenForTransactions);
			Assert.True(protectedBehavior.ServeBroadcasts);
		}
		finally { await global.DisposeAsync(); }
	}

	[Fact]
	public void LegacyPublicTorSettingIsIgnoredAndNotSaved()
	{
		var original = PersistentConfigManager.DefaultMainNetConfig;
		var json = PersistentConfigEncode.PersistentConfig(original).AsObject();
		json["UseTorForPublicData"] = true;
		var decoded = JsonDecoder.FromString(json.ToJsonString(), PersistentConfigDecode.CurrentPersistentConfig);
		Assert.Equal(original, decoded);
		Assert.DoesNotContain("UseTorForPublicData", PersistentConfigEncode.PersistentConfig(decoded!).ToJsonString());
	}

	[Theory]
	[InlineData(true)]
	[InlineData(false)]
	public void LegacyCoordinatorPublicTorSettingDoesNotAffectOnionPublishing(bool publishesOnion)
	{
		var json = Encode.WabiSabiConfig(new WabiSabiConfig("synthetic-config.json") { PublishAsOnionService = publishesOnion }).AsObject();
		json["UseTorForPublicData"] = true;
		var decoded = JsonDecoder.FromString(json.ToJsonString(), Decode.WabiSabiConfig("synthetic-config.json"));
		Assert.NotNull(decoded);
		Assert.Equal(publishesOnion, decoded.PublishAsOnionService);
		Assert.DoesNotContain("UseTorForPublicData", Encode.WabiSabiConfig(decoded).ToJsonString());
	}

	private static T Field<T>(object instance, string name) =>
		(T)instance.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(instance)!;
}
