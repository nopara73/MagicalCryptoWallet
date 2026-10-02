using System.Linq;
using System.Net;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using NBitcoin.Protocol;
using MagicalCryptoWallet.BitcoinP2p;
using MagicalCryptoWallet.Blockchain.Mempool;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Tests.Helpers;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Services;

public class MempoolTrafficTests
{
	[Fact]
	public void ConcurrentAnnouncementsReserveOneDownload()
	{
		var tracker = new TransactionRequestTracker();
		var now = DateTimeOffset.UtcNow;
		var accepted = 0;
		Parallel.For(0, 32, _ =>
		{
			if (tracker.TryRequest(uint256.One, Guid.NewGuid(), now)) { Interlocked.Increment(ref accepted); }
		});
		Assert.Equal(1, accepted);
	}

	[Fact]
	public void AnotherPeerCanTakeOverAfterTimeoutAndOldOwnerCannotReleaseItsRequest()
	{
		var tracker = new TransactionRequestTracker();
		var first = Guid.NewGuid();
		var second = Guid.NewGuid();
		var now = DateTimeOffset.UtcNow;
		Assert.True(tracker.TryRequest(uint256.One, first, now));
		Assert.False(tracker.TryRequest(uint256.One, second, now.AddSeconds(29)));
		Assert.True(tracker.TryRequest(uint256.One, second, now.AddSeconds(30)));
		tracker.Release(uint256.One, first);
		tracker.ReleaseOwner(first);
		Assert.False(tracker.TryRequest(uint256.One, first, now.AddSeconds(31)));
		tracker.ReleaseOwner(second);
		Assert.True(tracker.TryRequest(uint256.One, first, now.AddSeconds(31)));
	}

	[Fact]
	public void ReservationsAreBoundedAndReleasedOnNotFoundOrCompletion()
	{
		var tracker = new TransactionRequestTracker(capacity: 1);
		var peer = Guid.NewGuid();
		var now = DateTimeOffset.UtcNow;
		Assert.True(tracker.TryRequest(uint256.One, peer, now));
		Assert.False(tracker.TryRequest(uint256.Zero, peer, now));
		tracker.Release(uint256.One, peer);
		Assert.True(tracker.TryRequest(uint256.Zero, peer, now));
		tracker.Complete(uint256.Zero);
		Assert.True(tracker.TryRequest(uint256.One, peer, now));
	}

	[Fact]
	public void ReceiveOnlyPeersTreatWalletTransactionsLikeAllOtherPublicAnnouncements()
	{
		var service = new MempoolService(new EventBus());
		var transaction = BitcoinFactory.CreateSmartTransaction();
		Assert.True(service.TryAddToBroadcastStore(transaction));
		var receive = new P2pBehavior(service, serveBroadcasts: false);
		var broadcast = new P2pBehavior(service, listenForTransactions: false);
		var announcement = new InventoryVector(InventoryType.MSG_TX, transaction.GetHash());
		var first = IPEndPoint.Parse("203.0.113.1:8333");
		var second = IPEndPoint.Parse("203.0.113.2:8333");
		Assert.True(receive.ProcessInventoryVector(announcement, first));
		Assert.True(receive.ProcessInventoryVector(announcement, second));
		Assert.True(service.TryGetFromBroadcastStore(transaction.GetHash(), out var entry));
		Assert.False(entry.PropagationConfirmed.Task.IsCompleted);
		Assert.False(broadcast.ProcessInventoryVector(announcement, first));
		Assert.False(broadcast.ProcessInventoryVector(announcement, second));
		Assert.True(entry.PropagationConfirmed.Task.IsCompleted);
		Assert.False(Assert.IsType<P2pBehavior>(receive.Clone()).ServeBroadcasts);
	}

	[Fact]
	public void TransactionWithoutWitnessPublishesOnce()
	{
		var bus = new EventBus();
		var service = new MempoolService(bus);
		var received = 0;
		using var subscription = bus.Subscribe<NewTransactionInMempool>(_ => received++);
		var transaction = BitcoinFactory.CreateTransaction();
		Assert.Equal(transaction.GetHash(), transaction.GetWitHash());
		service.Process(transaction);
		service.Process(transaction);
		Assert.Equal(1, received);
	}

	[Fact]
	public void ReceivedWitnessTransactionCompletesBothReservationsAndPublishesOnce()
	{
		var bus = new EventBus();
		var service = new MempoolService(bus);
		var received = 0;
		using var subscription = bus.Subscribe<NewTransactionInMempool>(_ => received++);
		var transaction = BitcoinFactory.CreateTransaction();
		transaction.Inputs[0].WitScript = new WitScript(new byte[][] { [1, 2, 3] });
		var txid = transaction.GetHash();
		var witnessId = transaction.GetWitHash();
		Assert.NotEqual(txid, witnessId);
		service.Process(transaction);
		service.Process(transaction);
		Assert.True(service.IsProcessed(txid));
		Assert.True(service.IsProcessed(witnessId));
		Assert.Equal(1, received);
		var receive = new P2pBehavior(service, serveBroadcasts: false);
		Assert.False(receive.ProcessInventoryVector(new InventoryVector(InventoryType.MSG_WTX, witnessId), IPEndPoint.Parse("203.0.113.1:8333")));
	}
}
