using System.Net;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using NBitcoin.Protocol;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Services.NodesManagement;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Services;

public class P2pDiscoveryTests
{
	[Fact]
	public async Task OfflineStartupRetriesDiscoveryAfterCooldownAsync()
	{
		var network = new NetworkBuilder()
			.SetName($"offline-discovery-{Guid.NewGuid():N}")
			.SetNetworkSet(Bitcoin.Instance)
			.SetChainName(Network.RegTest.ChainName)
			.SetConsensus(Network.RegTest.Consensus)
			.SetGenesis(Convert.ToHexString(Network.RegTest.GetGenesis().ToBytes()))
			.AddDNSSeeds([new DNSSeedData("synthetic", "offline.invalid")])
			.BuildAndRegister();
		var dns = new OfflineDnsResolver();
		using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
		using var manager = new P2pConnectionManager(network, new EventBus(), dns, TimeSpan.FromSeconds(1));
		manager.Start(timeout.Token);
		while (dns.Attempts == 0) { await Task.Delay(10, timeout.Token); }
		var now = DateTimeOffset.UtcNow;
		await manager.ReevaluateConnectionsAsync(now + TimeSpan.FromMinutes(4), timeout.Token);
		Assert.Equal(1, dns.Attempts);
		await manager.ReevaluateConnectionsAsync(now + TimeSpan.FromMinutes(6), timeout.Token);
		Assert.Equal(2, dns.Attempts);
		await manager.ReevaluateConnectionsAsync(now + TimeSpan.FromMinutes(7), timeout.Token);
		Assert.Equal(2, dns.Attempts);
		Assert.Empty(manager.Nodes);
		timeout.Cancel();
	}

	private sealed class OfflineDnsResolver : IDnsResolver
	{
		private int _attempts;
		public int Attempts => Volatile.Read(ref _attempts);
		public Task<IPAddress[]> GetHostAddressesAsync(string host, CancellationToken cancellationToken)
		{
			Interlocked.Increment(ref _attempts);
			return Task.FromResult<IPAddress[]>([]);
		}
	}
}
