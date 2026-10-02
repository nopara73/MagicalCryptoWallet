using System;
using System.Collections.Concurrent;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using NBitcoin.Protocol;
using MagicalCryptoWallet.BitcoinP2p;
using MagicalCryptoWallet.Blockchain.Mempool;
using MagicalCryptoWallet.IntegrationTests.Infrastructure;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Services.NodesManagement;
using Xunit;

namespace MagicalCryptoWallet.IntegrationTests.SyncTests;

[Collection("Integration tests")]
public class PublicPeerTrafficTests(IntegrationTestFixture fixture)
{
	[Fact(Timeout = 90_000)]
	public async Task DirectReceiveOnlyPoolDownloadsBlocksAndReceivesMempoolAsync()
	{
		await using var environment = await RegTestEnvironment.CreateAsync(fixture);
		using var cancellation = CancellationTokenSource.CreateLinkedTokenSource(TestContext.Current.CancellationToken);
		cancellation.CancelAfter(TimeSpan.FromSeconds(60));
		var cachePath = Path.Combine(environment.WorkDir, "Peers-public.json");
		var now = DateTimeOffset.UtcNow;
		PeerAddressCache.Save(cachePath, [new PeerInfo(environment.BitcoinCoreNode.P2pEndPoint, "synthetic-regtest", 0,
			NodeServices.Network | NodeServices.NODE_WITNESS | NodeServices.NODE_COMPACT_FILTERS, 0, TimeSpan.Zero, now, now)], now);
		var bus = new EventBus();
		var mempool = new MempoolService(bus);
		using var pool = new P2pConnectionManager(environment.Network, bus, DnsResolver.Instance, TimeSpan.FromSeconds(5),
			options: new P2pConnectionOptions
			{
				TargetConnections = 1, MinimumCompactFilterNodes = 1, AllowBlockDownloads = true,
				AllowTransactionBroadcasts = false, RelayTransactions = true, PeerCacheFile = cachePath
			});
		pool.AddBehavior(new P2pBehavior(mempool, serveBroadcasts: false));
		pool.Start(cancellation.Token);
		var peer = await pool.GetSingleUseNodeAsync(cancellation.Token);
		var hash = await environment.RpcClient.GetBestBlockHashAsync(cancellation.Token);
		var block = await peer.GetBlockAsync(hash, cancellation.Token);
		Assert.NotNull(block);
		Assert.Equal(hash, block.GetHash());

		var received = new ConcurrentDictionary<uint256, int>();
		using var subscription = bus.Subscribe<NewTransactionInMempool>(e => received.AddOrUpdate(e.Transaction.GetHash(), 1, (_, n) => n + 1));
		using var key = new Key();
		var address = key.PubKey.GetAddress(ScriptPubKeyType.Segwit, environment.Network);
		var transactionId = await environment.RpcClient.SendToAddressAsync(address, Money.Coins(1), cancellationToken: cancellation.Token);
		while (!received.ContainsKey(transactionId)) { await Task.Delay(100, cancellation.Token); }
		Assert.Equal(1, received[transactionId]);
		Assert.True(mempool.IsProcessed(transactionId));
		Assert.DoesNotContain(peer.Node, P2pBehavior.GetNodesWillingToRelay(new FeeRate(100m)));
	}
}
