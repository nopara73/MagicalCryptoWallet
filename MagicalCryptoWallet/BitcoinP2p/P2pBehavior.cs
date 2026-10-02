using NBitcoin;
using NBitcoin.Protocol;
using NBitcoin.Protocol.Behaviors;
using System.Collections.Concurrent;
using System.Linq;
using System.Net;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Blockchain.Mempool;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.BitcoinP2p;

public class P2pBehavior : NodeBehavior
{
	private const int MaxInvSize = 50000;

	private static readonly ConcurrentDictionary<Node, FeeRate> PeerFeeFilters = new();
	private readonly ConcurrentDictionary<uint256, (InventoryVector Inventory, DateTimeOffset Expires)> _pending = new();
	private readonly Guid _requestOwner = Guid.NewGuid();
	private CancellationTokenSource? _lifetime;
	private Timer? _retryTimer;
	private int _requesting;

	public P2pBehavior(MempoolService mempoolService, bool listenForTransactions = true, bool serveBroadcasts = true)
	{
		MempoolService = mempoolService;
		ListenForTransactions = listenForTransactions;
		ServeBroadcasts = serveBroadcasts;
	}

	public MempoolService MempoolService { get; }
	public bool ListenForTransactions { get; }
	public bool ServeBroadcasts { get; }

	public static FeeRate? GetMinPeerFeeFilter() =>
		PeerFeeFilters.Select(x => x.Value).MinOrDefault();

	public static Node[] GetNodesWillingToRelay(FeeRate feeRate) =>
		PeerFeeFilters
			.Where(x => x.Key.IsConnected)
			.Where(x => x.Value <= feeRate)
			.Select(x => x.Key)
			.ToArray();

	protected override void AttachCore()
	{
		AttachedNode.MessageReceived += AttachedNode_MessageReceivedAsync;
		if (ServeBroadcasts) { PeerFeeFilters[AttachedNode] = new FeeRate(1m); }
		if (ListenForTransactions)
		{
			_lifetime = new CancellationTokenSource();
			var token = _lifetime.Token;
			var node = AttachedNode;
			_retryTimer = new Timer(_ => _ = RequestPendingAsync(node, token), null, TimeSpan.FromSeconds(1), TimeSpan.FromSeconds(1));
		}
	}

	protected override void DetachCore()
	{
		AttachedNode.MessageReceived -= AttachedNode_MessageReceivedAsync;
		PeerFeeFilters.TryRemove(AttachedNode, out _);
		_retryTimer?.Dispose();
		_lifetime?.Cancel();
		_lifetime?.Dispose();
		MempoolService.Requests.ReleaseOwner(_requestOwner);
		_pending.Clear();
	}

	private async void AttachedNode_MessageReceivedAsync(Node node, IncomingMessage message)
	{
		try
		{
			if (ServeBroadcasts && message.Message.Payload is GetDataPayload getDataPayload)
			{
				await ProcessGetDataAsync(node, getDataPayload).ConfigureAwait(false);
			}
			else if (ListenForTransactions && message.Message.Payload is TxPayload txPayload)
			{
				ProcessTx(txPayload);
			}
			else if (ServeBroadcasts && message.Message.Payload is FeeFilterPayload feeFilterPayload)
			{
				PeerFeeFilters[node] = feeFilterPayload.FeeRate;
			}
			else if (message.Message.Payload is InvPayload invPayload)
			{
				await ProcessInventoryAsync(node, invPayload).ConfigureAwait(false);
			}
			else if (ListenForTransactions && message.Message.Payload is NotFoundPayload notFound)
			{
				foreach (var inv in notFound.Inventory)
				{
					_pending.TryRemove(inv.Hash, out _);
					MempoolService.Requests.Release(inv.Hash, _requestOwner);
				}
			}
		}
		catch (OperationCanceledException ex)
		{
			Logger.LogDebug(ex);
		}
		catch (Exception ex)
		{
			Logger.LogInfo($"Ignoring {ex.GetType()}: {ex.Message}");
			Logger.LogDebug(ex);
		}
	}

	private async Task ProcessInventoryAsync(Node node, InvPayload invPayload)
	{
		if (invPayload.Inventory.Count > MaxInvSize) { return; }
		foreach (var inv in invPayload.Inventory)
		{
			if (ProcessInventoryVector(inv, node.RemoteSocketEndpoint))
			{
				if (_pending.Count < MaxInvSize) { _pending.TryAdd(inv.Hash, (inv, DateTimeOffset.UtcNow.AddMinutes(2))); }
			}
		}
		if (_lifetime is { } lifetime) { await RequestPendingAsync(node, lifetime.Token).ConfigureAwait(false); }
	}

	private async Task RequestPendingAsync(Node node, CancellationToken cancellationToken)
	{
		if (Interlocked.CompareExchange(ref _requesting, 1, 0) != 0) { return; }
		var getDataPayload = new GetDataPayload();
		try
		{
			if (!node.IsConnected || cancellationToken.IsCancellationRequested) { return; }
			var now = DateTimeOffset.UtcNow;
			foreach (var (hash, pending) in _pending)
			{
				if (MempoolService.IsProcessed(hash) || pending.Expires <= now)
				{
					_pending.TryRemove(hash, out _);
					MempoolService.Requests.Release(hash, _requestOwner);
				}
				else if (MempoolService.Requests.TryRequest(hash, _requestOwner, now))
				{
					getDataPayload.Inventory.Add(new InventoryVector(node.AddSupportedOptions(pending.Inventory.Type), hash));
				}
			}
			if (getDataPayload.Inventory.Count != 0)
			{
				await node.SendMessageAsync(getDataPayload).WaitAsync(cancellationToken).ConfigureAwait(false);
			}
		}
		catch (Exception ex)
		{
			foreach (var inv in getDataPayload.Inventory) { MempoolService.Requests.Release(inv.Hash, _requestOwner); }
			if (ex is not OperationCanceledException) { Logger.LogDebug(ex); }
		}
		finally
		{
			if (cancellationToken.IsCancellationRequested) { MempoolService.Requests.ReleaseOwner(_requestOwner); }
			Interlocked.Exchange(ref _requesting, 0);
		}
	}

	internal bool ProcessInventoryVector(InventoryVector inv, EndPoint remoteSocketEndpoint)
	{
		if ((inv.Type & ~InventoryType.MSG_WITNESS_FLAG) is InventoryType.MSG_TX or InventoryType.MSG_WTX)
		{
			if (ServeBroadcasts && MempoolService.TryGetFromBroadcastStore(inv.Hash, out TransactionBroadcastEntry? entry))
			{
				entry.ConfirmPropagationOnce(remoteSocketEndpoint);
				return false;
			}

			if (!ListenForTransactions) { return false; }

			// If we already processed it, then don't ask for it.
			if (MempoolService.IsProcessed(inv.Hash))
			{
				return false;
			}

			return true;
		}

		return false;
	}

	private async Task ProcessGetDataAsync(Node node, GetDataPayload payload)
	{
		if (payload.Inventory.Count > MaxInvSize)
		{
			Logger.LogDebug($"Received inventory too big. {nameof(MaxInvSize)}: {MaxInvSize}, Node: {node.RemoteSocketEndpoint}");
			return;
		}

		foreach (var inv in payload.Inventory.Where(inv => inv.Type.HasFlag(InventoryType.MSG_TX) || inv.Type.HasFlag(InventoryType.MSG_WTX)))
		{
			if (MempoolService.TryGetFromBroadcastStore(inv.Hash, out TransactionBroadcastEntry? entry)) // If we have the transaction to be broadcasted then broadcast it now.
			{
				if (!node.IsConnected)
				{
					Logger.LogDebug($"Could not serve transaction. Node ({node.RemoteSocketEndpoint}) is not connected anymore: {entry.TransactionId}.");
				}
				else
				{
					var txPayload = new TxPayload(entry.Transaction.Transaction);
					await node.SendMessageAsync(txPayload).ConfigureAwait(false);
					entry.BroadcastedTo(node.RemoteSocketEndpoint, node.Network);
					Logger.LogDebug($"Successfully served transaction to node ({node.RemoteSocketEndpoint}): {entry.TransactionId}.");
				}
			}
		}
	}

	private void ProcessTx(TxPayload payload)
	{
		Transaction transaction = payload.Object;
		transaction.PrecomputeHash(false, true);
		MempoolService.Process(transaction);
	}

	public override object Clone() => new P2pBehavior(MempoolService, ListenForTransactions, ServeBroadcasts);
}
