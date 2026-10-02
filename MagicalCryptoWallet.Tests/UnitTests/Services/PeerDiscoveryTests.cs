using System.IO;
using System.Linq;
using System.Net;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using NBitcoin.Protocol;
using MagicalCryptoWallet.BitcoinP2p;
using MagicalCryptoWallet.Blockchain.Mempool;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Services.NodesManagement;
using MagicalCryptoWallet.Tests.Helpers;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Services;

public class PeerDiscoveryTests
{
	[Fact]
	public void PeerReservationsKeepPublicAndWalletPoolsSeparate()
	{
		var reservations = new PeerConnectionRegistry();
		var endpoint = IPEndPoint.Parse("203.0.113.1:8333");
		var alias = new IPEndPoint(endpoint.Address.MapToIPv6(), endpoint.Port);
		Assert.True(reservations.TryReserve(endpoint, "public"));
		Assert.False(reservations.TryReserve(alias, "wallet"));
		Assert.False(reservations.TryReserve(new IPEndPoint(endpoint.Address, 18333), "wallet"));
		reservations.Release(endpoint, "wallet");
		Assert.True(reservations.IsReserved(alias));
		reservations.Release(alias, "public");
		Assert.True(reservations.TryReserve(endpoint, "wallet"));
	}

	[Fact]
	public void DiscoveryDeduplicatesPendingActiveAndRecentlyProbedPeers()
	{
		var now = DateTimeOffset.UtcNow;
		var endpoint = IPEndPoint.Parse("203.0.113.1:8333");
		var alias = new IPEndPoint(endpoint.Address.MapToIPv6(), endpoint.Port);
		var queue = new PeerDiscoveryQueue();
		queue.Enqueue([endpoint, alias, endpoint], now);
		Assert.Equal(1, queue.Count);
		Assert.True(queue.TryDequeue(out var selected));
		queue.Enqueue([alias], now);
		Assert.Equal(0, queue.Count);
		queue.Complete(selected, now, succeeded: true);
		queue.Enqueue([endpoint], now.AddHours(5));
		Assert.Equal(0, queue.Count);
		queue.Enqueue([alias], now.AddHours(6));
		Assert.Equal(1, queue.Count);
	}

	[Fact]
	public void FailedDiscoveryRetriesAfterCooldownAndQueueIsBounded()
	{
		var now = DateTimeOffset.UtcNow;
		var endpoints = Enumerable.Range(1, 20).Select(n => (EndPoint)new IPEndPoint(IPAddress.Parse($"203.0.113.{n}"), 8333)).ToArray();
		var queue = new PeerDiscoveryQueue(capacity: 2);
		queue.Enqueue(endpoints, now);
		Assert.Equal(2, queue.Count);
		Assert.True(queue.TryDequeue(out var first));
		Assert.True(queue.TryDequeue(out var second));
		queue.Complete(first, now, succeeded: false);
		queue.Complete(second, now, succeeded: false);
		queue.Enqueue([first, second], now.AddMinutes(4));
		Assert.Equal(0, queue.Count);
		queue.Enqueue([first, second], now.AddMinutes(5));
		Assert.Equal(2, queue.Count);
	}

	[Fact]
	public void PublicReceiveOnlyPoolRejectsBroadcastBehavior()
	{
		using var pool = new P2pConnectionManager(Network.RegTest, new EventBus(), DnsResolver.Instance, TimeSpan.FromSeconds(1),
			options: new P2pConnectionOptions { AllowTransactionBroadcasts = false, AllowBlockDownloads = true });
		Assert.Throws<InvalidOperationException>(() => pool.AddBehavior(new P2pBehavior(new MempoolService(new EventBus()))));
		pool.AddBehavior(new P2pBehavior(new MempoolService(new EventBus()), serveBroadcasts: false));
	}

	[Fact]
	public async Task BroadcastPoolCannotSupplyBlocksAsync()
	{
		using var pool = new P2pConnectionManager(Network.RegTest, new EventBus(), DnsResolver.Instance, TimeSpan.FromSeconds(1),
			options: new P2pConnectionOptions { AllowBlockDownloads = false });
		await Assert.ThrowsAsync<InvalidOperationException>(() => pool.GetSingleUseNodeAsync(CancellationToken.None));
	}

	[Fact]
	public void BroadcastRankingDoesNotPreferCompactFiltersAndKeepsPenalties()
	{
		using var broadcastPool = new P2pConnectionManager(Network.RegTest, new EventBus(), DnsResolver.Instance, TimeSpan.FromSeconds(1),
			options: new P2pConnectionOptions { MinimumCompactFilterNodes = 0 });
		using var publicPool = new P2pConnectionManager(Network.RegTest, new EventBus(), DnsResolver.Instance, TimeSpan.FromSeconds(1));
		var compact = NewPeer(1, DateTimeOffset.UtcNow);
		var ordinary = compact with { Services = compact.Services & ~NodeServices.NODE_COMPACT_FILTERS, Score = compact.Score - 30 };
		Assert.Equal(broadcastPool.GetPeerScore(compact), broadcastPool.GetPeerScore(ordinary));
		Assert.True(publicPool.GetPeerScore(compact) > publicPool.GetPeerScore(ordinary));
		Assert.True(broadcastPool.GetPeerScore(compact with { Score = compact.Score - 10 }) < broadcastPool.GetPeerScore(ordinary));
	}

	[Fact]
	public void BlockOnlyWalletPoolRetainsBroadcastBehaviorWithoutMempoolListening()
	{
		using var pool = new P2pConnectionManager(Network.RegTest, new EventBus(), DnsResolver.Instance, TimeSpan.FromSeconds(1),
			options: new P2pConnectionOptions { TargetConnections = 3, MinimumCompactFilterNodes = 0, RelayTransactions = false });
		var behavior = new P2pBehavior(new MempoolService(new EventBus()), listenForTransactions: false);
		pool.AddBehavior(behavior);
		Assert.False(Assert.IsType<P2pBehavior>(behavior.Clone()).ListenForTransactions);
	}

	[Fact]
	public async Task PeerCachePersistsFreshHintsAndBoundsItsSizeAsync()
	{
		var directory = await Common.GetEmptyWorkDirAsync();
		var path = Path.Combine(directory, "Peers.json");
		var now = DateTimeOffset.UtcNow;
		var peers = Enumerable.Range(1, 200).Select(n => NewPeer(n, now)).ToArray();
		PeerAddressCache.Save(path, peers.Concat([NewPeer(201, now.AddDays(-8)), NewPeer(202, now.AddDays(1))]), now);
		var loaded = PeerAddressCache.Load(path, now);
		Assert.Equal(128, loaded.Length);
		Assert.All(loaded, peer => Assert.True(peer.SupportsCompactFilters));
		Assert.Empty(PeerAddressCache.Load(path, now.AddDays(7)));
		Assert.Empty(Directory.GetFiles(directory, "*.tmp"));
	}

	[Fact]
	public async Task MalformedPeerCacheDoesNotPreventStartupAsync()
	{
		var directory = await Common.GetEmptyWorkDirAsync();
		var path = Path.Combine(directory, "Peers.json");
		await File.WriteAllTextAsync(path, "invalid-json");
		Assert.Empty(PeerAddressCache.Load(path, DateTimeOffset.UtcNow));
		await File.WriteAllTextAsync(path, new string(' ', 128 * 1_024 + 1));
		Assert.Empty(PeerAddressCache.Load(path, DateTimeOffset.UtcNow));
	}

	private static PeerInfo NewPeer(int n, DateTimeOffset seen) => new(new IPEndPoint(IPAddress.Parse($"203.0.113.{n}"), 8333),
		"synthetic", 70016, NodeServices.Network | NodeServices.NODE_COMPACT_FILTERS | NodeServices.NODE_WITNESS, 0, TimeSpan.Zero, seen, seen);
}
