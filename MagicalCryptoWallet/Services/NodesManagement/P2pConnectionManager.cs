using NBitcoin;
using NBitcoin.Protocol;
using NBitcoin.Protocol.Behaviors;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.Diagnostics;
using System.Linq;
using System.Net;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.BitcoinP2p;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;
using static MagicalCryptoWallet.Services.Workers;

namespace MagicalCryptoWallet.Services.NodesManagement;

public record P2pNodeClient(
	Node Node,
	double Timeout,
	Action<int> IncreaseTimeout,
	Action<P2pConnectionManager.MisbehaviorType> Error)
{
	public async Task<Block?> GetBlockAsync(uint256 blockHash, CancellationToken cancellationToken)
	{
		try
		{
			using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(Timeout));
			using var lts = CancellationTokenSource.CreateLinkedTokenSource(cts.Token, cancellationToken);
			var block = await Node.DownloadBlockAsync(blockHash, lts.Token).ConfigureAwait(false);

			if (block is null)
			{
				return null;
			}

			// Validate block
			if (!block.Check())
			{
				Error(P2pConnectionManager.MisbehaviorType.ProvidedInvalidData);
				return null;
			}

			Logger.LogInfo($"Block ({block.GetCoinbaseHeight()}) downloaded: {block.GetHash()}.");
			IncreaseTimeout(-1);
			return block;
		}
		catch (Exception ex) when (ex is OperationCanceledException or TimeoutException)
		{
			IncreaseTimeout(+1);
			// It could be a slow connection and not a misbehaving node.
			Error(P2pConnectionManager.MisbehaviorType.TimedOutDownloadingBlock);
		}
		catch (InvalidOperationException ex)
		{
			Logger.LogWarning(ex);
			Error(P2pConnectionManager.MisbehaviorType.ProvidedInvalidData);
		}
		catch (Exception)
		{
			// ignored
		}

		return null;
	}
}

public delegate Task<P2pNodeClient> P2pNodeProvider(CancellationToken cancellationToken);

/// <summary>Snapshot of currently connected P2P nodes.</summary>
public delegate ImmutableArray<Node> P2pNodeListProvider();

public class P2pConnectionManager : IDisposable
{
	private const double RotationScoreThreshold = 1.1;
	private const int DefaultCrawlerCount = 4;
	private const int MaxPeersPerNetgroup = 3;

	private static readonly TimeSpan ReconnectCooldown = TimeSpan.FromMinutes(5);
	private static readonly TimeSpan QuickDisconnectThreshold = TimeSpan.FromSeconds(30);
	private static readonly TimeSpan MaintainInterval = TimeSpan.FromSeconds(6);
	private static readonly TimeSpan RotateInterval = TimeSpan.FromMinutes(3);
	private static readonly TimeSpan CrawlerConnectionTimeout = TimeSpan.FromSeconds(15);
	private static readonly TimeSpan CrawlerHarvestTimeout = TimeSpan.FromSeconds(4);

	private readonly Network _network;
	private readonly List<NodeBehavior> _templateBehaviors = [];
	private readonly EventBus _eventBus;
	private readonly IDnsResolver _dnsResolver;
	private readonly TimeSpan _connectionTimeout;
	private readonly int _crawlerCount;
	private readonly EndPoint? _torSocks5;
	private P2pConnectionOptions _options;
	private readonly PeerConnectionRegistry _reservations;
	private readonly string _owner = Guid.NewGuid().ToString("N");
	private readonly PeerDiscoveryQueue _discoveryQueue = new();
	private readonly ConcurrentDictionary<EndPoint, PeerInfo> _cachedPeers = new();
	private volatile bool _discoveryNeeded = true;
	private DateTimeOffset _lastDnsSeed;
	private int _dnsSeedRunning;
	private DateTimeOffset _lastCacheSave;
	private int _started;

	private readonly ConcurrentDictionary<EndPoint, (Node Node, PeerInfo PeerInfo, DateTimeOffset ConnectedAt)> _connectedNodes = new();
	private readonly ConcurrentDictionary<EndPoint, DateTimeOffset> _connectionAttempts = new();
	private readonly ComposedDisposable _disposables = new();

	private MailboxProcessor<CrawlerMessage>[]? _crawlers;
	private MailboxProcessor<CoordinatorMessage>? _discoveryCoordinator;

	private int _isReevaluating;
	private DateTimeOffset _lastMaintainTime;
	private DateTimeOffset _lastRotateTime;

	private int _timeoutsCounter;
	private int _currentTimeoutSeconds = 16;

	private bool _isDisposed;

	public P2pConnectionManager(
		Network network,
		EventBus eventBus,
		IDnsResolver dnsResolver,
		TimeSpan connectionTimeout,
		int crawlerCount = DefaultCrawlerCount,
		EndPoint? torSocks5 = null,
		P2pConnectionOptions? options = null,
		PeerConnectionRegistry? reservations = null)
	{
		_network = network;
		_eventBus = eventBus;
		_dnsResolver = dnsResolver;
		_connectionTimeout = connectionTimeout;
		_crawlerCount = crawlerCount;
		_torSocks5 = torSocks5;
		_options = options ?? new P2pConnectionOptions();
		_reservations = reservations ?? new PeerConnectionRegistry();
		ArgumentOutOfRangeException.ThrowIfLessThan(crawlerCount, 1);
		ConfigurePeerTargets(_options.TargetConnections, _options.MinimumCompactFilterNodes);
	}

	public void ConfigurePeerTargets(int targetConnections, int minimumCompactFilterNodes)
	{
		ArgumentOutOfRangeException.ThrowIfLessThan(targetConnections, 1);
		ArgumentOutOfRangeException.ThrowIfNegative(minimumCompactFilterNodes);
		ArgumentOutOfRangeException.ThrowIfGreaterThan(minimumCompactFilterNodes, targetConnections);
		_options = _options with { TargetConnections = targetConnections, MinimumCompactFilterNodes = minimumCompactFilterNodes };
	}

	public ImmutableArray<Node> Nodes => _connectedNodes.Values.Select(x => x.Node).Where(x => x.IsConnected).ToImmutableArray();

	public void AddBehavior(NodeBehavior behavior)
	{
		if (!_options.AllowBlockDownloads && behavior is P2pBehavior)
		{
			throw new InvalidOperationException("Public synchronization peers cannot handle wallet transactions.");
		}
		_templateBehaviors.Add(behavior);
		foreach (var (_, node) in _connectedNodes)
		{
			node.Node.Behaviors.Add(behavior);
		}
	}

	public void Start(CancellationToken cancellationToken)
	{
		if (Interlocked.Exchange(ref _started, 1) != 0) { return; }
		if (_options.PeerCacheFile is { } cacheFile)
		{
			foreach (var peer in PeerAddressCache.Load(cacheFile, DateTimeOffset.UtcNow).Where(p => CanConnectToEndpoint(p.Endpoint))) { _cachedPeers[peer.Endpoint] = peer; }
		}
		_crawlers = Enumerable
			.Range(0, _crawlerCount)
			.Select(n =>
				Spawn($"{_options.Name}-{_owner}-crawler-{n}",
					EventDriven(
						Unit.Instance,
						CreateCrawler(n)),
					capacity: 1_000,
					cancellationToken: cancellationToken))
			.ToArray();

		_disposables.AddRange(_crawlers);

		_discoveryCoordinator = Spawn(
			$"{_options.Name}-{_owner}-discovery",
			Service("Bitcoin Node Discovery Service",
				EventDriven(
					new CrawlingCoordinationState(Peers: _cachedPeers.ToImmutableDictionary(), BusyCrawlers: ImmutableHashSet<int>.Empty),
					CreateDiscovery(_crawlers))),
			cancellationToken: cancellationToken);
		_discoveryCoordinator.DisposeUsing(_disposables);

		_ = Task.Run(async () =>
		{
			try
			{
				await ReevaluateConnectionsAsync(DateTimeOffset.UtcNow, cancellationToken).ConfigureAwait(false);
				await SeedFromDnsAsync(cancellationToken).ConfigureAwait(false);
			}
			catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested) { }
		}, cancellationToken);

		_eventBus.Subscribe<Tick>(async void (_) =>
		{
			await ReevaluateConnectionsAsync(DateTimeOffset.UtcNow, cancellationToken).ConfigureAwait(false);
			_discoveryCoordinator?.Post(new DiscoveryTickMessage());
		}).DisposeUsing(_disposables);

		_eventBus.Subscribe<NodeDisconnectedQuickly>(e =>
			ReportMisbehavior(e.EndPoint, MisbehaviorType.DisconnectedQuickly)).DisposeUsing(_disposables);

		_eventBus.Subscribe<MisbehavingNodeDetected>(e =>
			ReportMisbehavior(e.EndPoint, MisbehaviorType.ProvidedInvalidData)).DisposeUsing(_disposables);

		_eventBus.Subscribe<NodeTimeoutDownloadingBlock>(e =>
			ReportMisbehavior(e.EndPoint, MisbehaviorType.TimedOutDownloadingBlock)).DisposeUsing(_disposables);
	}

	public async Task ReevaluateConnectionsAsync(DateTimeOffset now, CancellationToken cancellationToken)
	{
		if (Interlocked.CompareExchange(ref _isReevaluating, 1, 0) != 0)
		{
			return;
		}

		try
		{
			PurgeDisconnectedNodes();

			var count = _connectedNodes.Count;
			if ((now - _lastMaintainTime >= MaintainInterval && count < _options.TargetConnections) || count == 0)
			{
				_lastMaintainTime = now;
				await ConnectToBestPeersAsync(cancellationToken).ConfigureAwait(false);
			}

			if ((now - _lastRotateTime >= RotateInterval && count >= _options.TargetConnections) ||
			    (now - _lastRotateTime >= TimeSpan.FromSeconds(4) && _connectedNodes.Count(x => x.Value.PeerInfo.SupportsCompactFilters) < _options.MinimumCompactFilterNodes))
			{
				_lastRotateTime = now;
				await RotateToBetterPeersAsync(cancellationToken).ConfigureAwait(false);
			}
		}
		catch (Exception e)
		{
			Logger.LogWarning(e.Message);
		}
		finally
		{
			Interlocked.Exchange(ref _isReevaluating, 0);
		}
	}

	public async Task<P2pNodeClient> GetSingleUseNodeAsync(CancellationToken cancellationToken)
	{
		if (!_options.AllowBlockDownloads) { throw new InvalidOperationException("Wallet-selected blocks must use protected peers."); }
		while (!cancellationToken.IsCancellationRequested)
		{
			var nodes = Nodes.Where(n => n.CanServeBlocks).ToArray();

			if (nodes.Length == 0)
			{
				await Task.Delay(TimeSpan.FromMilliseconds(100), cancellationToken).ConfigureAwait(false);
				continue;
			}

			var node = nodes.RandomElement(RandomnessProviders.Secure);

			if (node is not null && node.IsConnected)
			{
				return new P2pNodeClient(
					node,
					GetCurrentTimeout(),
					UpdateTimeout,
					misbehavior =>
					{
						DisconnectNode(node, misbehavior);
						ReportMisbehavior(node.RemoteSocketEndpoint, misbehavior);
					});
			}

			Logger.LogTrace("Selected node is null or disconnected.");
			await Task.Delay(10, cancellationToken).ConfigureAwait(false);
		}

		cancellationToken.ThrowIfCancellationRequested();
		throw new InvalidOperationException("Failed to retrieve a connected node.");
	}

	public void DisconnectNode(Node node, MisbehaviorType misbehaviorType)
	{
		var minimumPeers = Math.Max(1, _options.MinimumCompactFilterNodes);
		var shouldDisconnect = (misbehaviorType, Nodes.Length) switch
		{
			(MisbehaviorType.ProvidedInvalidData, _) => true,
			(MisbehaviorType.Unknown, _) => true,
			(MisbehaviorType.TimedOutDownloadingBlock, var count) when count > minimumPeers => true,
			(_, var count) when count <= minimumPeers => false,
			(_, _) => node.SupportsCompactFilters,
		};

		if (!shouldDisconnect)
		{
			return;
		}

		var disconnectionReason = misbehaviorType switch
		{
			MisbehaviorType.ProvidedInvalidData => "Reason: it provided invalid data",
			MisbehaviorType.TimedOutDownloadingBlock => "Reason: it took too long to download a block",
			_ => ""
		};
		Logger.LogInfo($"Node {node.RemoteSocketEndpoint} disconnected. {disconnectionReason}");
		node.DisconnectAsync();
	}

	private double GetCurrentTimeout()
	{
		// More permissive timeout if few nodes are connected to avoid exhaustion.
		return Nodes.Length < 3
			? Math.Min(_currentTimeoutSeconds * 1.5, 600)
			: _currentTimeoutSeconds;
	}

	/// <summary>
	/// Current timeout used when downloading a block from the remote node. It is defined in seconds.
	/// </summary>
	private void UpdateTimeout(int addition)
	{
		_timeoutsCounter += addition;

		var timeout = _currentTimeoutSeconds;

		// If it times out 2 times in a row then increase the timeout.
		if (_timeoutsCounter >= 2)
		{
			_timeoutsCounter = 0;
			timeout = (int)Math.Round(timeout * 1.5);
		}
		else if (_timeoutsCounter <= -3) // If it does not time out 3 times in a row, lower the timeout.
		{
			_timeoutsCounter = 0;
			timeout = (int)Math.Round(timeout * 0.7);
		}

		// Sanity check
		var minTimeout = _network == Network.Main ? 3 : 2;

		if (timeout < minTimeout)
		{
			timeout = minTimeout;
		}
		else if (timeout > 600)
		{
			timeout = 600;
		}

		_currentTimeoutSeconds = timeout;
		Logger.LogInfo($"Current timeout value used on block download is: {timeout} seconds.");
	}

	private async Task<PeerInfo[]> GetDiscoveredPeersAsync(CancellationToken cancellationToken)
	{
		if (_discoveryCoordinator is null)
		{
			return [];
		}

		return await _discoveryCoordinator.PostAndReplyAsync<PeerInfo[]>(
			reply => new GetPeersMessage(reply),
			cancellationToken).ConfigureAwait(false);
	}

	private async Task ConnectToBestPeersAsync(CancellationToken cancellationToken)
	{
		var filterNodeCount = _connectedNodes.Values
			.Count(n => n.Node.IsConnected && n.PeerInfo.SupportsCompactFilters);

		var filterNodesNeeded = Math.Max(0, _options.MinimumCompactFilterNodes - filterNodeCount);
		var totalNeeded = _options.TargetConnections - _connectedNodes.Count;
		var availablePeers = await GetAvailablePeersAsync(cancellationToken).ConfigureAwait(false);

		var filterPeers = availablePeers
			.Where(p => p.SupportsCompactFilters)
			.OrderByDescending(p => p.Score)
			.Take(filterNodesNeeded * 2)
			.ToArray();

		var otherPeers = availablePeers.Except(filterPeers)
			.OrderByDescending(p => p.Score)
			.Take(totalNeeded * 2)
			.ToArray();

		var peers = filterPeers
			.Concat(otherPeers)
			.DistinctBy(p => p.Endpoint)
			.Take(totalNeeded)
			.Shuffle()
			.ToArray();

		if (peers.Length > 0)
		{
			Logger.LogTrace($"Connecting to {peers.Length} peers (current: {_connectedNodes.Count}).");
			await Task.WhenAll(peers.Select(p => ConnectToPeerAsync(p, cancellationToken))).ConfigureAwait(false);
		}
	}

	private async Task<PeerInfo[]> GetAvailablePeersAsync(CancellationToken cancellationToken)
	{
		var connectedKeys = _connectedNodes.Keys.ToHashSet();
		var now = DateTimeOffset.UtcNow;
		var cooldownEndpoints = _connectionAttempts
			.Where(kvp => now - kvp.Value < ReconnectCooldown)
			.Select(kvp => kvp.Key)
			.ToHashSet();

		// Track netgroup counts for connected nodes
		var netgroupCounts = _connectedNodes.Values
			.GroupBy(n => GetNetgroup(n.PeerInfo.Endpoint))
			.ToDictionary(g => g.Key, g => g.Count());

		var peers = await GetDiscoveredPeersAsync(cancellationToken).ConfigureAwait(false);
		var availablePeers = peers
			.Select(p => (Peer: p, NetGroup: GetNetgroup(p.Endpoint)))
			.Where(p => IsAvailable(p.Peer.Endpoint))
			.GroupBy(p => p.NetGroup)
			.SelectMany(g => g.OrderByDescending(p => p.Peer.Score).Take(MaxPeersPerNetgroup - netgroupCounts.GetValueOrDefault(g.Key, 0)))
			.Select(p => p.Peer)
			.ToArray();

		return availablePeers;

		bool IsAvailable(EndPoint endpoint) =>
			!connectedKeys.Contains(endpoint) &&
			!cooldownEndpoints.Contains(endpoint) && !_reservations.IsReserved(endpoint);
	}

	private static string GetNetgroup(EndPoint endpoint)
	{
		if (endpoint is IPEndPoint ipEp)
		{
			var ip = ipEp.Address;

			if (ip.IsIPv4MappedToIPv6)
			{
				ip = ip.MapToIPv4();
			}

			if (ip.AddressFamily == System.Net.Sockets.AddressFamily.InterNetwork)
			{
				var b = ip.GetAddressBytes();
				return $"v4:{Convert.ToHexString(b.AsSpan(0,2))}"; // /16 fallback. Bitcoin Core uses an AS map instead
			}

			if (ip.AddressFamily == System.Net.Sockets.AddressFamily.InterNetworkV6)
			{
				var b = ip.GetAddressBytes(); // 16 bytes
				return $"v6:{Convert.ToHexString(b.AsSpan(0, 4))}";
			}
		}

		if (endpoint is DnsEndPoint dns && IsOnionHost(dns.Host))
		{
			return "onion:*";
		}

		return $"other:{endpoint.GetType().Name}";
	}

	static bool IsOnionHost(string host) =>
		host.EndsWith(".onion", StringComparison.OrdinalIgnoreCase);

	private async Task ConnectToPeerAsync(PeerInfo peerInfo, CancellationToken cancellationToken)
	{
		if (!TryReserveConnectionAttempt(peerInfo.Endpoint))
		{
			return;
		}

		Node? node = null;
		try
		{
			using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
			timeoutCts.CancelAfter(_connectionTimeout);

			var connParams = new NodeConnectionParameters
			{
				ConnectCancellation = timeoutCts.Token,
				IsRelay = _options.RelayTransactions,
				UserAgent = Constants.UserAgents[Random.Shared.Next(Constants.UserAgents.Length)]
			};

			foreach (var behavior in _templateBehaviors)
			{
				connParams.TemplateBehaviors.Add(behavior);
			}

			if (_torSocks5 is { } torEndpoint)
			{
				connParams.TemplateBehaviors.Add(new SocksSettingsBehavior(torEndpoint, onlyForOnionHosts: false,
					networkCredential: null, streamIsolation: true));
			}

			node = await Node.ConnectAsync(_network, peerInfo.Endpoint, connParams)
				.ConfigureAwait(false);
			await node.VersionHandshakeAsync(timeoutCts.Token).ConfigureAwait(false);

			if (node.State != NodeState.HandShaked)
			{
				node.DisconnectAsync();
				return;
			}

			node.Disconnected += OnNodeDisconnected;

			var actualPeer = CreatePeerInfo(node, peerInfo.Endpoint, peerInfo.ConnectionTime);
			_discoveryCoordinator?.Post(new PeerDiscoveredMessage(actualPeer));
			if (_connectedNodes.TryAdd(peerInfo.Endpoint, (node, actualPeer, DateTimeOffset.UtcNow)))
			{
				Logger.LogDebug($"Connected to peer {peerInfo.Endpoint} (score: {peerInfo.Score:F1}, services: {peerInfo.Services.AsCsv()}). Total connected peers: {_connectedNodes.Count}.");
				_eventBus.Publish(new P2pNodeAdded(peerInfo.Endpoint, node));
			}
			else
			{
				DisconnectNode(node);
			}
		}
		catch (OperationCanceledException)
		{
		}
		catch (Exception ex)
		{
			Logger.LogDebug($"Failed to connect to {peerInfo.Endpoint}: {ex.Message}");
			ReportMisbehavior(peerInfo.Endpoint, MisbehaviorType.FailedToConnect);
		}
		finally
		{
			if (!_connectedNodes.ContainsKey(peerInfo.Endpoint))
			{
				node?.DisconnectAsync();
				_reservations.Release(peerInfo.Endpoint, _owner);
			}
		}
	}

	private bool TryReserveConnectionAttempt(EndPoint endpoint)
	{
		if (_connectedNodes.ContainsKey(endpoint))
		{
			Logger.LogDebug($"Already connected to peer: {endpoint}");
			return false;
		}

		if (_connectionAttempts.TryGetValue(endpoint, out var last) &&
		    DateTimeOffset.UtcNow - last < ReconnectCooldown)
		{
			Logger.LogDebug($"Connection attempt to peer: {endpoint} skipped (too often)");
			return false;
		}

		if (!CanConnectToEndpoint(endpoint) || !_reservations.TryReserve(endpoint, _owner)) { return false; }
		_connectionAttempts[endpoint] = DateTimeOffset.UtcNow;
		return true;
	}

	private void OnNodeDisconnected(Node node)
	{
		if (_connectedNodes.TryRemove(PeerConnectionRegistry.Normalize(node.Peer.Endpoint), out var removed))
		{
			_reservations.Release(removed.PeerInfo.Endpoint, _owner);
			node.Disconnected -= OnNodeDisconnected;
			var connectionDuration = DateTimeOffset.UtcNow - removed.ConnectedAt;
			Logger.LogDebug($"Peer {node.Peer.Endpoint} (score: {removed.PeerInfo.Score:F1}) disconnected after {connectionDuration.TotalSeconds:F1}s. Total connected peers: {_connectedNodes.Count}.");

			if (connectionDuration < QuickDisconnectThreshold)
			{
				_eventBus.Publish(new NodeDisconnectedQuickly(removed.PeerInfo.Endpoint, node));
			}

			_eventBus.Publish(new P2pNodeRemoved(removed.PeerInfo.Endpoint, node));
		}
	}

	private async Task RotateToBetterPeersAsync(CancellationToken cancellationToken)
	{
		var rankedConnectedNodes = _connectedNodes.Values
			.Where(n => n.Node.IsConnected)
			.OrderBy(x => x.PeerInfo.Score)
			.ToArray();

		if (rankedConnectedNodes is not [var worstConnectedNode, ..])
		{
			return;
		}

		var availablePeers = await GetAvailablePeersAsync(cancellationToken).ConfigureAwait(false);
		var bestDiscoveredNode = availablePeers
			.OrderByDescending(p => p.Score)
			.ThenByDescending(p => p.SupportsCompactFilters)
			.FirstOrDefault();

		if (bestDiscoveredNode is null)
		{
			Logger.LogTrace("There is no best peer candidate");
			return;
		}

		var bestDiscoveredScore = bestDiscoveredNode.Score;

		if (bestDiscoveredScore > worstConnectedNode.PeerInfo.Score * RotationScoreThreshold)
		{
			Logger.LogInfo($"Replacing peer {worstConnectedNode.PeerInfo.Endpoint} (score: {worstConnectedNode.PeerInfo.Score:F1}) with {bestDiscoveredNode.Endpoint} (score: {bestDiscoveredScore:F1})");

			if (_connectedNodes.TryRemove(worstConnectedNode.PeerInfo.Endpoint, out var nodeToRemove))
			{
				DisconnectNode(nodeToRemove.Node);
				_eventBus.Publish(new P2pNodeRemoved(nodeToRemove.PeerInfo.Endpoint, nodeToRemove.Node));
			}

			await ConnectToPeerAsync(bestDiscoveredNode, cancellationToken).ConfigureAwait(false);
		}
		else
		{
			Logger.LogDebug($"Peer candidate {bestDiscoveredNode.Endpoint} (score: {bestDiscoveredScore:F1}) is not significantly better than our worst peer (score: {worstConnectedNode.PeerInfo.Score:F1}). Skipping rotation.");
		}
	}

	private void PurgeDisconnectedNodes()
	{
		var dead = _connectedNodes.Where(kv => !kv.Value.Node.IsConnected).ToArray();

		foreach (var (key, (node, _, _)) in dead)
		{
			if (_connectedNodes.TryRemove(key, out _))
			{
				_reservations.Release(key, _owner);
				node.Disconnected -= OnNodeDisconnected;
				_eventBus.Publish(new P2pNodeRemoved(key, node));
			}
		}
	}

	private void DisconnectNode(Node node)
	{
		node.Disconnected -= OnNodeDisconnected;
		node.DisconnectAsync();
		_reservations.Release(node.Peer.Endpoint, _owner);
	}

	private void DisconnectAll()
	{
		foreach (var (_, (node, _, _)) in _connectedNodes)
		{
			DisconnectNode(node);
		}
		_connectedNodes.Clear();
	}

	public void Dispose()
	{
		if (_isDisposed)
		{
			return;
		}

		_isDisposed = true;
		SavePeerCache();
		DisconnectAll();
		_disposables.Dispose();
	}

	private void ReportMisbehavior(EndPoint endpoint, MisbehaviorType misbehavior) =>
		_discoveryCoordinator?.Post(new NodeMisbehaveMessage(PeerConnectionRegistry.Normalize(endpoint), misbehavior));

	#region Discovery

	public enum MisbehaviorType
	{
		FailedToConnect,
		DisconnectedQuickly,
		ProvidedInvalidData,
		TimedOutDownloadingBlock,
		Unknown
	}

	private abstract record CoordinatorMessage;
	private record HarvestedEndpointsMessage(EndPoint[] Endpoints) : CoordinatorMessage;
	private record PeerDiscoveredMessage(PeerInfo PeerInfo) : CoordinatorMessage;
	private record NodeMisbehaveMessage(EndPoint Endpoint, MisbehaviorType Behavior) : CoordinatorMessage;
	private record DiscoveryTickMessage : CoordinatorMessage;
	private record CrawlFinishedMessage(int CrawlerIndex, EndPoint Endpoint, bool Succeeded) : CoordinatorMessage;
	private record GetPeersMessage(IReplyChannel<PeerInfo[]> ReplyChannel) : CoordinatorMessage;

	private abstract record CrawlerMessage;
	private record CrawlMessage(EndPoint EndPoint) : CrawlerMessage;

	private record CrawlingCoordinationState(
		ImmutableDictionary<EndPoint, PeerInfo> Peers,
		ImmutableHashSet<int> BusyCrawlers);

	private MessageHandler<CoordinatorMessage, CrawlingCoordinationState> CreateDiscovery(
		MailboxProcessor<CrawlerMessage>[] crawlers) =>
		(msg, state, token) => HandleCoordinatorMessageAsync(crawlers, msg, state, token);

	private Task<CrawlingCoordinationState> HandleCoordinatorMessageAsync(
		MailboxProcessor<CrawlerMessage>[] crawlers,
		CoordinatorMessage msg,
		CrawlingCoordinationState state,
		CancellationToken cancellationToken)
	{
		switch (msg)
		{
			case HarvestedEndpointsMessage(Endpoints: var endpoints):
				_discoveryQueue.Enqueue(endpoints.Where(CanConnectToEndpoint), DateTimeOffset.UtcNow);
				break;

			case PeerDiscoveredMessage(PeerInfo: var peer):
				var updatedPeer = state.Peers.TryGetValue(peer.Endpoint, out var existingPeer)
					? peer with { DiscoveredAt = existingPeer.DiscoveredAt, Score = double.Min(70, peer.Score + 2) }
					: peer;

				state = state with { Peers = state.Peers.SetItem(peer.Endpoint, updatedPeer) };

				_cachedPeers[peer.Endpoint] = updatedPeer;
				if (state.Peers.Count > 256)
				{
					foreach (var obsolete in state.Peers.Values.Where(p => !_connectedNodes.ContainsKey(p.Endpoint)).OrderBy(p => p.LastSeen).Take(state.Peers.Count - 256))
					{
						state = state with { Peers = state.Peers.Remove(obsolete.Endpoint) };
						_cachedPeers.TryRemove(obsolete.Endpoint, out _);
					}
				}
				if (state.Peers.Count % 8 == 0 || DateTimeOffset.UtcNow - _lastCacheSave >= TimeSpan.FromMinutes(5)) { SavePeerCache(); }
				break;
			case CrawlFinishedMessage finished:
				_discoveryQueue.Complete(finished.Endpoint, DateTimeOffset.UtcNow, finished.Succeeded);
				state = state with { BusyCrawlers = state.BusyCrawlers.Remove(finished.CrawlerIndex) };
				break;

			case NodeMisbehaveMessage(Endpoint: var offendingEndpoint, Behavior: var behavior):
				if (state.Peers.TryGetValue(offendingEndpoint, out var offendingNode))
				{
					state = behavior switch
					{
						MisbehaviorType.TimedOutDownloadingBlock when offendingNode.Score > 30 =>
							Punish(offendingEndpoint, offendingNode, behavior),
						MisbehaviorType.DisconnectedQuickly when offendingNode.Score > 30 =>
							Punish(offendingEndpoint, offendingNode, behavior),
						MisbehaviorType.FailedToConnect when offendingNode.Score > 30 =>
							Punish(offendingEndpoint, offendingNode, behavior),
						_ =>
							Remove(offendingEndpoint)
					};
				}
				break;

			case GetPeersMessage(ReplyChannel: var replyChannel):
				replyChannel.Reply(state.Peers.Values.ToArray());
				break;
			case DiscoveryTickMessage:
				if (_discoveryNeeded && _discoveryQueue.Count == 0 && state.BusyCrawlers.Count == 0 && DateTimeOffset.UtcNow - _lastDnsSeed > ReconnectCooldown)
				{
					_ = Task.Run(() => SeedFromDnsAsync(cancellationToken), cancellationToken);
				}
				break;
		}

		_discoveryNeeded = NeedsDiscovery(state);
		if (_discoveryNeeded)
		{
			for (var index = 0; index < crawlers.Length; index++)
			{
				if (state.BusyCrawlers.Contains(index)) { continue; }
				if (!_discoveryQueue.TryDequeue(out var endpoint)) { break; }
				if (crawlers[index].Post(new CrawlMessage(endpoint)))
				{
					state = state with { BusyCrawlers = state.BusyCrawlers.Add(index) };
				}
				else { _discoveryQueue.Complete(endpoint, DateTimeOffset.UtcNow, succeeded: false); }
			}
		}
		return Task.FromResult(state);

		CrawlingCoordinationState Punish(EndPoint offendingEndpoint, PeerInfo offendingNode, MisbehaviorType misbehaviorType)
		{
			var newPeerInfo = offendingNode with { Score = offendingNode.Score - 10 };
			_cachedPeers[offendingEndpoint] = newPeerInfo;
			Logger.LogDebug($"Peer {offendingNode.Endpoint} was punished for {misbehaviorType}. Score {offendingNode.Score:F1} -> {newPeerInfo.Score:F1}.");
			return state with
			{
				Peers = state.Peers.SetItem(offendingEndpoint, offendingNode with {Score = offendingNode.Score - 10})
			};
		}

		CrawlingCoordinationState Remove(EndPoint offendingEndpoint)
		{
			_cachedPeers.TryRemove(offendingEndpoint, out _);
			return state with { Peers = state.Peers.Remove(offendingEndpoint) };
		}
	}

	private bool NeedsDiscovery(CrawlingCoordinationState state)
	{
		var now = DateTimeOffset.UtcNow;
		var available = state.Peers.Values.Where(p => !_reservations.IsReserved(p.Endpoint) &&
			(!_connectionAttempts.TryGetValue(p.Endpoint, out var attempt) || now - attempt >= ReconnectCooldown)).ToArray();
		return Nodes.Length < _options.TargetConnections ||
			_connectedNodes.Values.Count(p => p.Node.IsConnected && p.PeerInfo.SupportsCompactFilters) < _options.MinimumCompactFilterNodes ||
			available.Length < Math.Max(8, _options.TargetConnections * 2) ||
			available.Count(p => p.SupportsCompactFilters) < _options.MinimumCompactFilterNodes;
	}

	private void SavePeerCache()
	{
		if (_options.PeerCacheFile is { } cacheFile)
		{
			PeerAddressCache.Save(cacheFile, _cachedPeers.Values, DateTimeOffset.UtcNow);
			_lastCacheSave = DateTimeOffset.UtcNow;
		}
	}

	private bool CanConnectToEndpoint(EndPoint endpoint) => endpoint.IsValid() && !endpoint.IsI2P() &&
		!(endpoint is IPEndPoint { Address: var ip } && ip.IsCjdns()) && (_torSocks5 is not null || !endpoint.IsTor());

	private static PeerInfo CreatePeerInfo(Node node, EndPoint endpoint, TimeSpan connectionTime)
	{
		var now = DateTimeOffset.UtcNow;
		var version = node.PeerVersion;
		return new PeerInfo(endpoint, version.UserAgent ?? "Unknown", version.Version, version.Services, version.StartHeight, connectionTime, now, now);
	}

	private MessageHandler<CrawlerMessage, Unit> CreateCrawler(int crawlerIndex) =>
		async (msg, state, token) => await HandleCrawlerMessageAsync(msg, state, crawlerIndex, token).ConfigureAwait(false);

	private async Task<Unit> HandleCrawlerMessageAsync(
		CrawlerMessage msg,
		Unit state,
		int crawlerIndex,
		CancellationToken cancellationToken)
	{
		switch (msg)
		{
			case CrawlMessage(var endpoint):
				Node? node = null;
				var probeOwner = $"{_owner}-probe-{crawlerIndex}";
				try
				{
					node = await VisitEndpointAsync(endpoint, probeOwner, cancellationToken).ConfigureAwait(false);
					if (node is not null)
					{
						await HarvestAddressesAsync(node, cancellationToken).ConfigureAwait(false);
					}
				}
				finally
				{
					node?.DisconnectAsync();
					_reservations.Release(endpoint, probeOwner);
					_discoveryCoordinator?.Post(new CrawlFinishedMessage(crawlerIndex, endpoint, node is not null));
				}
				break;
		}

		return state;
	}

	private async Task<Node?> VisitEndpointAsync(EndPoint endpoint, string probeOwner, CancellationToken cancellationToken)
	{
		if (!CanConnectToEndpoint(endpoint) || !_reservations.TryReserve(endpoint, probeOwner))
		{
			return null;
		}

		using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
		var crawlerConnectionTimeout = endpoint is DnsEndPoint dnsEndPoint && IsOnionHost(dnsEndPoint.Host)
			? CrawlerConnectionTimeout * 2
			: CrawlerConnectionTimeout;
		timeoutCts.CancelAfter(crawlerConnectionTimeout);

		var connParams = new NodeConnectionParameters
		{
			ConnectCancellation = timeoutCts.Token,
			IsRelay = false,
			UserAgent = Constants.UserAgents[Random.Shared.Next(Constants.UserAgents.Length)]
		};

		if (_torSocks5 is { } torEndpoint)
		{
			connParams.TemplateBehaviors.Add(new SocksSettingsBehavior(torEndpoint, onlyForOnionHosts: false, streamIsolation:false, networkCredential:null));
		}
		else if (endpoint.IsTor())
		{
			// Cannot connect to .onion endpoint without Tor
			return null;
		}

		Node? node = null;
		var sw = Stopwatch.StartNew();
		try
		{
			node = await Node.ConnectAsync(_network, endpoint, connParams).ConfigureAwait(false);
			await node.VersionHandshakeAsync(timeoutCts.Token).ConfigureAwait(false);

			sw.Stop();

			if (node.State != NodeState.HandShaked)
			{
				_discoveryCoordinator?.Post(new NodeMisbehaveMessage(endpoint, MisbehaviorType.FailedToConnect));
				node.DisconnectAsync();
				return null;
			}

			var peer = CreatePeerInfo(node, endpoint, sw.Elapsed);

			Logger.LogDebug($"Connected to endpoint '{endpoint}'");
			_discoveryCoordinator?.Post(new PeerDiscoveredMessage(peer));
		}
		catch (Exception e)
		{
			Logger.LogTrace($"Failed to connect to endpoint '{endpoint}' (requested cancellation: {cancellationToken.IsCancellationRequested})", e);

			_discoveryCoordinator?.Post(new NodeMisbehaveMessage(endpoint, MisbehaviorType.FailedToConnect));
			node?.DisconnectAsync();
			return null;
		}
		return node;
	}

	private async Task HarvestAddressesAsync(Node node, CancellationToken cancellationToken)
	{
		var tcs = new TaskCompletionSource<EndPoint[]>(TaskCreationOptions.RunContinuationsAsynchronously);

		using var timeoutCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken);
		timeoutCts.CancelAfter(CrawlerHarvestTimeout);
		var token = timeoutCts.Token;
		var _ = token.Register(() => tcs.TrySetCanceled(token));

		node.MessageReceived += OnMessage;
		try
		{
			await node.SendMessageAsync(new GetAddrPayload()).ConfigureAwait(false);
			var harvestedEndpoints = await tcs.Task.ConfigureAwait(false);
			_discoveryCoordinator?.Post(new HarvestedEndpointsMessage(harvestedEndpoints));
		}
		catch (OperationCanceledException) when (token.IsCancellationRequested)
		{
		}
		finally
		{
			node.MessageReceived -= OnMessage;
		}

		return;

		void OnMessage(object? _, IncomingMessage e)
		{
			if (e.Message.Payload is not AddrPayload addr)
			{
				return;
			}

			var harvested = new List<EndPoint>();
			foreach (var a in addr.Addresses.OrderByDescending(x => x.Services.HasFlag(NodeServices.NODE_COMPACT_FILTERS)))
			{
				if (a.Endpoint is { } ep)
				{
					harvested.Add(ep);
				}
			}

			tcs.TrySetResult(harvested.ToArray());
		}
	}

	private async Task SeedFromDnsAsync(CancellationToken cancellationToken)
	{
		if (!_discoveryNeeded || Interlocked.CompareExchange(ref _dnsSeedRunning, 1, 0) != 0) { return; }
		_lastDnsSeed = DateTimeOffset.UtcNow;
		Logger.LogInfo("Seeding from DNS...");
		try
		{
			_discoveryCoordinator?.Post(new HarvestedEndpointsMessage(_network.SeedNodes.Select(x => x.Endpoint).ToArray()));
			var hosts = _network.DNSSeeds.Select(x => x.Host).Distinct().ToArray();
			var maximumRounds = _dnsResolver is DnsSocksResolver ? 16 : 1;
			for (var round = 0; round < maximumRounds && _discoveryNeeded; round++)
			{
				var tasks = hosts.Shuffle().Select(GetAddressesFromDnsAsync);
				await foreach (var task in Task.WhenEach(tasks).WithCancellation(cancellationToken))
				{
					var result = await task.ConfigureAwait(false);
					if (result.IsOk)
					{
						var endpoints = result.Value.Select(x => (EndPoint)new IPEndPoint(x, _network.DefaultPort)).ToArray();
						_discoveryCoordinator?.Post(new HarvestedEndpointsMessage(endpoints));
					}
				}
				if (_discoveryNeeded && round + 1 < maximumRounds) { await Task.Delay(TimeSpan.FromSeconds(1), cancellationToken).ConfigureAwait(false); }
			}
		}
		catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested) { }
		catch (Exception ex) { Logger.LogDebug("Peer seeding failed.", ex); }
		finally { Interlocked.Exchange(ref _dnsSeedRunning, 0); }

		async Task<Result<IPAddress[], Exception>> GetAddressesFromDnsAsync(string host)
		{
			try { return await _dnsResolver.GetHostAddressesAsync(host, cancellationToken).ConfigureAwait(false); }
			catch (Exception ex) { return ex; }
		}
	}

	#endregion
}

public static class Extensions
{
	public static string AsCsv(this NodeServices ns)
	{
		(NodeServices, string)[] flagNames =
		[
			(NodeServices.Network, "Blocks"),
			(NodeServices.GetUTXO, "UTXO"),
			(NodeServices.NODE_BLOOM, "Bloom Filters"),
			(NodeServices.NODE_COMPACT_FILTERS, "Compact Filters"),
			(NodeServices.NODE_NETWORK_LIMITED, "Limited Network"),
			(NodeServices.NODE_WITNESS, "Witness"),
			((NodeServices)2048, "P2P v2")
		];

		var nsCopy = ns;
		var supportedServices = new List<string>(7);

		foreach (var (flag, name) in flagNames)
		{
			if (ns.HasFlag(flag))
			{
				supportedServices.Add(name);
				nsCopy &= ~flag;
			}
		}

		if (nsCopy > 0)
		{
			supportedServices.Add(((long)nsCopy).ToString());
		}

		return string.Join(" | ", supportedServices);
	}
}
