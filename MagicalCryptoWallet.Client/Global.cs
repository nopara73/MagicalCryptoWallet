using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.IO;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using NBitcoin.Protocol;
using Nito.AsyncEx;
using MagicalCryptoWallet.BitcoinP2p;
using MagicalCryptoWallet.Blockchain.BlockFilters;
using MagicalCryptoWallet.Blockchain.Blocks;
using MagicalCryptoWallet.Blockchain.Mempool;
using MagicalCryptoWallet.Blockchain.TransactionBroadcasting;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Client.Rpc;
using MagicalCryptoWallet.Discoverability;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.FeeRateEstimation;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Io;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Rpc;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Services.NodesManagement;
using MagicalCryptoWallet.Services.Terminate;
using MagicalCryptoWallet.Stores;
using MagicalCryptoWallet.Tor;
using MagicalCryptoWallet.Tor.Control;
using MagicalCryptoWallet.Tor.StatusChecker;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Client.Banning;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;
using MagicalCryptoWallet.WabiSabi.Client.RoundStateAwaiters;
using MagicalCryptoWallet.WabiSabi.Models;
using MagicalCryptoWallet.Wallets;
using MagicalCryptoWallet.Wallets.Exchange;
using MagicalCryptoWallet.WebClients.MagicalCryptoWallet;
using static MagicalCryptoWallet.Services.Workers;
using ChainHeight = MagicalCryptoWallet.Models.Height.ChainHeight;

namespace MagicalCryptoWallet.Client;

public class Global
{
	/// <remarks>Use this variable as a guard to prevent touching <see cref="_stoppingCts"/> that might have already been disposed.</remarks>
	private volatile bool _disposeRequested;

	public Global(string dataDir, Config config)
	{
		DataDir = dataDir;
		Config = config;
		TorSettings = new TorSettings(
			DataDir,
			distributionFolderPath: EnvironmentHelpers.GetFullBaseDirectory(),
			terminateOnExit: Config.TerminateTorOnExit,
			torMode: Config.UseTor,
			socksPort: config.TorSocksPort,
			controlPort: config.TorControlPort,
			torFolder: config.TorFolder,
			bridges: config.TorBridges,
			owningProcessId: Environment.ProcessId,
			log: Config.LogModes.Contains(LogMode.File));

		EventBus = new EventBus();
		Status = new StatusContainer(EventBus, installOnClose: Config.DownloadNewVersion);
		Status.DisposeUsing(_disposables);

		HostedServices = new HostedServices();
		HostedServices.DisposeUsing(_disposables);

		_mempoolService = new MempoolService(EventBus);
		FilterHeaders = new FilterHeaderChain();
		var networkWorkFolderPath = Path.Combine(DataDir, "BitcoinStore", Network.ToString());
		var fileSystemBlockRepository = new FileSystemBlockRepository(Path.Combine(networkWorkFolderPath, "Blocks"), Network);

		TransactionStore = new AllTransactionStore(networkWorkFolderPath, Network);
		TransactionStore.DisposeUsing(_disposables);

		FilterStore = new FilterStore(Path.Combine(networkWorkFolderPath, "IndexStore"), Network, FilterHeaders, EventBus);
		FilterStore.DisposeUsing(_disposables);

		ExternalSourcesHttpClientFactory = BuildHttpClientFactory();
		PublicSourcesHttpClientFactory = new DirectHttpClientFactory();

		var p2PDataDir = GetBitcoinP2PNetworkDirectory();
		_blockHeaders = ConfigureBlockHeaderChain(p2PDataDir);

		_publicConnectionManager = ConfigureNodeConnectionManager(publicSynchronization: true);
		_p2pConnectionManager = Config.UseTor != TorMode.Disabled
			? ConfigureNodeConnectionManager(publicSynchronization: false)
			: _publicConnectionManager;
		var cpfpProvider = ConfigureCpfpInfoProvider();
		var blockProvider = ConfigureBlockProvider(_publicConnectionManager, fileSystemBlockRepository);

		var walletFactory = Wallet.CreateFactory(
			Config.Network,
			FilterStore,
			TransactionStore,
			FilterHeaders,
			_mempoolService,
			Config.ServiceConfiguration,
			blockProvider,
			EventBus,
			cpfpProvider);

		var walletDirectories = new WalletDirectories(Config.Network, DataDir);
		WalletSession = new WalletSession(Config.Network, walletDirectories, walletFactory, () => GetPeerCount() > 0, RecoverStorageAsync);

		var broadcasters = CreateBroadcasters(p2PNodeListProvider: () => _p2pConnectionManager.Nodes, _mempoolService);
		TransactionBroadcaster = new TransactionBroadcaster(broadcasters.ToArray(), _mempoolService);

		Scheme = new Scheme(this);

		_ticker = new Timer(_ => EventBus.Publish(new Tick(DateTime.UtcNow)));
		_ticker.DisposeUsing(_disposables);

		_stoppingCts.DisposeUsing(_disposables);
		if (Config.TryGetCoordinatorUri(out var coordinatorUri)) { RegisterCoinJoinComponents(coordinatorUri); }
	}

	private readonly AsyncLock _initializationAsyncLock = new();
	private readonly AsyncLock _networkInitializationLock = new();
	private readonly Lock _initializationGate = new();
	private Task? _initializationTask;
	private bool _synchronizerStarted;
	private readonly CancellationTokenSource _stoppingCts = new();

	private readonly P2pConnectionManager _p2pConnectionManager;
	private readonly P2pConnectionManager _publicConnectionManager;
	private readonly PeerConnectionRegistry _peerReservations = new();
	private TorManager? _torManager;
	private CoinPrison? _coinPrison;
	private readonly ConcurrentChain _blockHeaders;
	private readonly Timer _ticker;
	private readonly ComposedDisposable _disposables = new();
	private readonly MempoolService _mempoolService;
	private readonly ComposedAsyncDisposable _asyncDisposables = new();

	public StatusContainer Status { get; }
	public string DataDir { get; }
	public TorSettings TorSettings { get; }

	public FilterHeaderChain FilterHeaders { get; }
	public FilterStore FilterStore { get; }
	public AllTransactionStore TransactionStore { get; }
	public IHttpClientFactory ExternalSourcesHttpClientFactory { get; }
	public IHttpClientFactory PublicSourcesHttpClientFactory { get; }
	public Config Config { get; }
	public WalletSession WalletSession { get; }
	public TransactionBroadcaster TransactionBroadcaster { get; }
	public HostedServices HostedServices { get; }
	public Network Network => Config.Network;
	public JsonRpcServer? RpcServer { get; private set; }
	public Uri? OnionServiceUri { get; private set; }
	public EventBus EventBus { get; }
	public Scheme Scheme { get; }

	private string GetBitcoinP2PNetworkDirectory() => Path.Combine(DataDir, "BitcoinP2pNetwork");

	private BlockProvider ConfigureBlockProvider(P2pConnectionManager p2pConnectionManager, FileSystemBlockRepository fileSystemBlockRepository)
	{
		var fileSystemBlockProvider = BlockProviders.FileSystemBlockProvider(fileSystemBlockRepository);
		var p2PBlockProvider = BlockProviders.P2pBlockProvider(async cancellationToken =>
		{
			p2pConnectionManager.Start(_stoppingCts.Token);
			return await p2pConnectionManager.GetSingleUseNodeAsync(cancellationToken).ConfigureAwait(false);
		});

		BlockProvider[] blockProviders = [fileSystemBlockProvider, p2PBlockProvider];

		return BlockProviders.CachedBlockProvider(
			BlockProviders.ComposedBlockProvider(blockProviders),
			fileSystemBlockRepository);
	}

	private ConcurrentChain ConfigureBlockHeaderChain(string p2PDataDir)
	{
		var blockHeadersFilePath = Path.Combine(p2PDataDir, $"BlockHeaders{Network}.dat");

		if (Network == Network.RegTest)
		{
			DeleteBlockHeadersFile(blockHeadersFilePath);
			return new ConcurrentChain(Network);
		}

		var blockHeaders = Result<byte[], Exception>
			.Catch(() => File.SafelyReadAllBytes(blockHeadersFilePath))
			.Match(
				bytes => bytes switch
				{
					[] => new ConcurrentChain(Network),
					_ => new ConcurrentChain(bytes, Network)
				},
				_ => new ConcurrentChain(Network));

		return blockHeaders;
	}

	private static void DeleteBlockHeadersFile(string blockHeadersFilePath)
	{
		foreach (var path in new[] { blockHeadersFilePath, $"{blockHeadersFilePath}.old", $"{blockHeadersFilePath}.new" })
		{
			if (File.Exists(path))
			{
				File.Delete(path);
			}
		}
	}

	private P2pConnectionManager ConfigureNodeConnectionManager(bool publicSynchronization)
	{
		if (Network == Network.Main)
		{
			if (Network.DNSSeeds is List<DNSSeedData> dnsSeeds)
			{
				AddDnsSeed(dnsSeeds, "petertodd.net", "seed.btc.petertodd.net");
				AddDnsSeed(dnsSeeds, "sprovoost.nl", "seed.bitcoin.sprovoost.nl");
				AddDnsSeed(dnsSeeds, "emzy.de", "dnsseed.emzy.de");
				AddDnsSeed(dnsSeeds, "wiz.biz", "seed.bitcoin.wiz.biz");
				AddDnsSeed(dnsSeeds, "achownodes.xyz", "seed.mainnet.achownodes.xyz");
			}
		}
		if (Network == Bitcoin.Instance.Signet)
		{
			if (Network.DNSSeeds is List<DNSSeedData> dnsSeeds)
			{
				AddDnsSeed(dnsSeeds, "sprovoost.nl", "seed.signet.bitcoin.sprovoost.nl");
				AddDnsSeed(dnsSeeds, "achownodes.xyz", "seed.signet.achownodes.xyz");
			}
		}
		if (Network == Network.RegTest)
		{
			if (Network.SeedNodes is List<NetworkAddress> addresses)
			{
				if (!addresses.Any(a => a.Endpoint.Equals(new IPEndPoint(IPAddress.Loopback, Network.DefaultPort))))
				{
					addresses.Add(new NetworkAddress(IPAddress.Loopback, Network.DefaultPort));
				}
			}
		}

		if (Network == Network.Main)
		{
			var extras = new[]
			{
				"[::ffff:89.58.60.208]:8333", "141.8.29.139:8333", "176.126.73.74:8333", "51.37.212.201:8333",
				"[::ffff:109.224.84.149]:8333", "[::ffff:123.202.192.214]:8333", "[::ffff:130.180.58.210]:8333",
				"[::ffff:144.2.65.179]:8333", "[::ffff:158.174.102.68]:8333", "[::ffff:158.248.16.134]:8333",
				"[::ffff:176.198.90.204]:8333", "[::ffff:176.9.150.253]:8333", "[::ffff:178.192.9.193]:8333",
				"[::ffff:178.26.46.97]:8333", "[::ffff:184.181.44.154]:8333", "[::ffff:217.92.137.136]:8333",
				"[::ffff:218.146.42.163]:8333", "[::ffff:24.134.6.165]:8333", "[::ffff:45.130.58.202]:8333",
				"[::ffff:45.41.51.43]:8333", "[::ffff:46.226.18.135]:8333", "[::ffff:46.229.238.187]:8333",
				"[::ffff:46.59.13.35]:8333", "[::ffff:47.181.150.194]:8333", "[::ffff:5.161.244.95]:8333",
				"[::ffff:62.177.111.78]:8333", "[::ffff:64.130.52.77]:8333", "[::ffff:71.185.186.173]:8333",
				"[::ffff:80.253.94.252]:8333", "[::ffff:82.67.127.46]:8333", "[::ffff:84.196.182.49]:8333",
				"[::ffff:86.242.20.49]:8333", "[::ffff:86.254.150.8]:8333", "[::ffff:87.236.195.198]:8333",
				"[::ffff:95.90.138.145]:8333", "179.51.86.34:8333", "188.63.47.92:8333",
				"54.248.26.73:8333", "82.67.161.60:8333", "92.252.69.140:8333", "92.98.3.189:8333",
				"[::ffff:109.202.209.123]:8333", "[::ffff:121.99.109.132]:8333", "[::ffff:13.48.242.195]:8333",
				"[::ffff:137.175.247.16]:8333", "[::ffff:141.195.182.138]:8333", "[::ffff:141.224.209.201]:8333",
				"[::ffff:15.134.155.244]:8333", "[::ffff:15.222.135.69]:8333", "[::ffff:159.196.59.9]:8333",
				"[::ffff:167.235.98.250]:8333", "[::ffff:174.165.47.165]:8333", "[::ffff:176.34.50.242]:8333",
				"[::ffff:178.26.219.209]:8333", "[::ffff:18.102.201.234]:8333", "[::ffff:181.115.88.2]:8333",
				"[::ffff:188.34.193.226]:8333", "[::ffff:194.160.169.63]:8333", "[::ffff:194.42.111.175]:8333",
				"[::ffff:203.12.2.113]:8333", "[::ffff:213.144.146.33]:8333", "[::ffff:3.23.202.194]:8333",
				"[::ffff:3.24.243.206]:8333", "[::ffff:38.172.231.13]:8333", "[::ffff:43.200.189.153]:8333",
				"[::ffff:43.202.209.174]:8333", "[::ffff:45.142.17.140]:8333", "[::ffff:47.130.216.204]:8333",
				"[::ffff:5.56.248.2]:8333", "[::ffff:50.106.24.94]:8333", "[::ffff:51.158.54.195]:8333",
				"[::ffff:51.44.200.138]:8333", "[::ffff:52.74.41.162]:8333", "[::ffff:54.246.85.218]:8333",
				"[::ffff:56.125.205.1]:8333", "[::ffff:56.125.249.13]:8333", "[::ffff:56.126.1.10]:8333",
				"[::ffff:56.126.33.251]:8333", "[::ffff:65.109.125.160]:8333", "[::ffff:68.103.11.30]:8333",
				"[::ffff:68.231.1.158]:8333", "[::ffff:71.179.175.122]:8333", "[::ffff:72.210.34.27]:8333",
				"[::ffff:72.253.193.231]:8333", "[::ffff:76.31.239.16]:8333", "[::ffff:77.162.78.194]:8333",
				"[::ffff:79.160.240.207]:8333", "[::ffff:82.66.107.156]:8333", "[::ffff:85.0.91.69]:8333",
				"[::ffff:85.3.52.172]:8333", "[::ffff:88.153.65.113]:8333", "[::ffff:89.217.193.252]:8333",
				"[::ffff:89.56.142.128]:8333", "[::ffff:89.56.206.21]:8333", "[::ffff:90.11.72.52]:8333",
				"[::ffff:90.189.215.153]:8333", "[::ffff:92.148.116.20]:8333", "[::ffff:93.56.5.69]:8333",
				"[::ffff:93.89.130.246]:8333"
			};

			if (Network.SeedNodes is List<NetworkAddress> addresses)
			{
				var existing = addresses.Select(x => PeerConnectionRegistry.Normalize(x.Endpoint)).ToHashSet();
				addresses.AddRange(extras.Select(IPEndPoint.Parse).Where(x => existing.Add(PeerConnectionRegistry.Normalize(x))).Select(x => new NetworkAddress(x)));
			}
		}

		var torEndpoint = Config.UseTor != TorMode.Disabled && !publicSynchronization
			? TorSettings.SocksEndpoint : null;
		IDnsResolver dnsResolver = torEndpoint is not null
			? new DnsSocksResolver(torEndpoint){ StreamIsolation = true }
			: DnsResolver.Instance;

		var canBroadcast = !publicSynchronization || Config.UseTor == TorMode.Disabled;
		var manager = new P2pConnectionManager(
			Network,
			EventBus,
			dnsResolver,
			TimeSpan.FromSeconds(15),
			torSocks5: torEndpoint,
			options: new P2pConnectionOptions
			{
				Name = publicSynchronization ? "public" : "wallet",
				TargetConnections = publicSynchronization ? 6 : Network.MinBroadcastNodes + 2,
				MinimumCompactFilterNodes = publicSynchronization ? 5 : 0,
				RelayTransactions = !publicSynchronization || !Config.BlockOnlyMode,
				AllowBlockDownloads = publicSynchronization,
				AllowTransactionBroadcasts = canBroadcast,
				PeerCacheFile = Path.Combine(GetBitcoinP2PNetworkDirectory(), publicSynchronization ? "Peers-public.json" : "Peers-wallet.json")
			},
			reservations: _peerReservations);

		manager.AddBehavior(new P2pBehavior(_mempoolService,
			listenForTransactions: publicSynchronization && !Config.BlockOnlyMode,
			serveBroadcasts: canBroadcast));

		manager.DisposeUsing(_disposables);
		return manager;

		static void AddDnsSeed(List<DNSSeedData> seeds, string name, string host)
		{
			if (!seeds.Any(seed => seed.Host.Equals(host, StringComparison.OrdinalIgnoreCase))) { seeds.Add(new DNSSeedData(name, host)); }
		}
	}

	private HttpClientFactory BuildHttpClientFactory(HttpClientHandlerConfiguration? config = null) =>
		Config.UseTor != TorMode.Disabled
			? new OnionHttpClientFactory(TorSettings.SocksEndpoint.ToUri("socks5"), config)
			: new HttpClientFactory(config);

	private void ConfigureFeeRateUpdater(CancellationToken cancellationToken)
	{
		var blockFeeProvider = FeeRateProviders.BlockAsync(PublicSourcesHttpClientFactory);
		var mempoolSpaceFeeProvider = FeeRateProviders.MempoolSpaceAsync(PublicSourcesHttpClientFactory);
		var blockstreamInfoFeeProvider = FeeRateProviders.BlockstreamAsync(PublicSourcesHttpClientFactory);
		FeeRateProvider feeRateProvider = Config.FeeRateEstimationProvider.ToLower() switch
		{
			"blockxyz" => FeeRateProviders.Composed([blockFeeProvider, mempoolSpaceFeeProvider, blockstreamInfoFeeProvider]),
			"mempoolspace" => FeeRateProviders.Composed([mempoolSpaceFeeProvider, blockstreamInfoFeeProvider]),
			"blockstreaminfo" => FeeRateProviders.Composed([blockstreamInfoFeeProvider, mempoolSpaceFeeProvider]),
			"" or "none" => FeeRateProviders.NoneAsync(),
			var providerName => throw new ArgumentException( $"Not supported fee rate estimations provider '{providerName}'. Default: '{Constants.DefaultFeeRateEstimationProvider}'")
		};

		var feeRateUpdater = Spawn("FeeRateUpdater",
			Service("Mining Fee Rate Updater",
				Periodically(
					TimeSpan.FromMinutes(15),
					FeeRateEstimations.Empty,
					FeeRateEstimationUpdater.CreateUpdater(feeRateProvider, EventBus))), cancellationToken);
		feeRateUpdater.DisposeUsing(_disposables);
		EventBus.Subscribe<Tick>(_ => feeRateUpdater.Post(new FeeRateEstimationUpdater.UpdateMessage()))
			.DisposeUsing(_disposables);
	}

	private async Task ConfigureSynchronizerAsync(CancellationToken cancellationToken)
	{
		var tip = FilterStore.GetTip()!.Header;
		var synchronizationState = new FilterSynchronizationState(_blockHeaders, FilterHeaders, tip.Height, EventBus);
		_publicConnectionManager.AddBehavior(new BlockHeadersChainBehavior(_blockHeaders, FilterHeaders, EventBus));
		_publicConnectionManager.AddBehavior(new CompactFilterBehavior(synchronizationState, _blockHeaders, EventBus));
		var filtersProvider = FilterProviders.CreateBitcoinP2pFilterProvider(FilterHeaders, _blockHeaders, synchronizationState);
		var (_, resume, serviceLoop) = Continuously(Synchronizer.CreateFilterGenerator(filtersProvider, FilterStore, FilterHeaders, EventBus));
		await resume().ConfigureAwait(false);
		Spawn("Synchronizer", Service("Magical Crypto Wallet Index-Based Synchronizer", serviceLoop), cancellationToken)
			.DisposeUsing(_disposables);
	}

	private void ConfigureExchangeRateUpdater(CancellationToken cancellationToken)
	{
		var mempoolSpaceExchangeProvider = ExchangeRateProviders.MempoolSpaceAsync(PublicSourcesHttpClientFactory);
		var blockchainInfoExchangeProvider = ExchangeRateProviders.BlockchainInfoAsync(PublicSourcesHttpClientFactory);
		var coinGeckoExchangeProvider = ExchangeRateProviders.CoinGeckoAsync(PublicSourcesHttpClientFactory);
		var geminiExchangeProvider = ExchangeRateProviders.GeminiAsync(PublicSourcesHttpClientFactory);
		ExchangeRateProvider exchangeRateProvider = Config.ExchangeRateProvider.ToLower() switch
		{
			"mempoolspace" => ExchangeRateProviders.Composed([mempoolSpaceExchangeProvider, blockchainInfoExchangeProvider, coinGeckoExchangeProvider, geminiExchangeProvider ]),
			"blockchaininfo" => ExchangeRateProviders.Composed([blockchainInfoExchangeProvider, mempoolSpaceExchangeProvider, coinGeckoExchangeProvider, geminiExchangeProvider]),
			"coingecko" => ExchangeRateProviders.Composed([coinGeckoExchangeProvider, mempoolSpaceExchangeProvider, blockchainInfoExchangeProvider, geminiExchangeProvider]),
			"gemini" => ExchangeRateProviders.Composed([geminiExchangeProvider, blockchainInfoExchangeProvider, mempoolSpaceExchangeProvider, coinGeckoExchangeProvider]),
			"" or "none" => ExchangeRateProviders.NoneAsync(),
			var providerName => throw new ArgumentException( $"Not supported exchange rate provider '{providerName}'. Default: '{Constants.DefaultExchangeRateProvider}'")
		};

		var exchangeFeeRateUpdater = Spawn(ExchangeRateUpdater.ServiceName,
				Service("Exchange Rate Updater",
					Periodically(
						TimeSpan.FromMinutes(20),
						0m,
						ExchangeRateUpdater.CreateExchangeRateUpdater(exchangeRateProvider, EventBus))), cancellationToken);
		exchangeFeeRateUpdater.DisposeUsing(_disposables);
		EventBus.Subscribe<Tick>(_ => exchangeFeeRateUpdater.Post(new ExchangeRateUpdater.UpdateMessage()))
			.DisposeUsing(_disposables);
	}

	private void ConfigureMagicalCryptoWalletUpdater(CancellationToken cancellationToken)
	{
		if (Network == Network.RegTest) { return; }
		Uri[] relayUrls = [new ("wss://relay.primal.net"), new("wss://nos.lol"), new("wss://nostr.mom")];
		var nostrClientFactory = () => NostrClientFactory.Create(relayUrls, (EndPoint?)null);

		// The feature is disabled on linux at the moment because we install Magical Crypto Wallet as a Debian package.
		var installerDownloader = !Config.DownloadNewVersion
			? ReleaseDownloader.AutoDownloadOff()
			: RuntimeInformation.IsOSPlatform(OSPlatform.Linux) && !PlatformInformation.IsDebianBasedOS()
				? ReleaseDownloader.ForUnsupportedLinuxDistributions()
				: ReleaseDownloader.ForOfficiallySupportedOSes(PublicSourcesHttpClientFactory, EventBus);

		var magicalcryptowalletVersionUpdater = Spawn("UpdateManager",
			Service("Magical Crypto Wallet Version AutoUpdater",
				Periodically(
					TimeSpan.FromHours(12),
					Unit.Instance,
					UpdateManager.CreateUpdater(nostrClientFactory, installerDownloader, EventBus))), cancellationToken);
		magicalcryptowalletVersionUpdater.DisposeUsing(_disposables);
		EventBus.Subscribe<Tick>(_ => magicalcryptowalletVersionUpdater.Post(new UpdateManager.UpdateMessage()))
			.DisposeUsing(_disposables);
	}

	private CpfpInfoProvider ConfigureCpfpInfoProvider()
	{
		var cpfpUpdater = Spawn("CpfpInfoProvider",
			Service("External Cpfp Info provider",
				EventDriven(
					Unit.Instance,
					Network == Network.RegTest
					? CpfpInfoUpdater.CreateForRegTest()
					: CpfpInfoUpdater.Create(ExternalSourcesHttpClientFactory, Network, EventBus))),
			_stoppingCts.Token);
		cpfpUpdater.DisposeUsing(_disposables);
		EventBus.Subscribe<FilterProcessed>(_ => cpfpUpdater.Post(new CpfpInfoMessage.UpdateMessage()))
			.DisposeUsing(_disposables);
		return new CpfpInfoProvider(cpfpUpdater);
	}

	private async Task<bool> InitializeBitcoinStoreAsync(CancellationToken cancellationToken)
	{
		try
		{
			await PrepareStoresAsync(cancellationToken).ConfigureAwait(false);
			await WalletSession.InitializeAsync(cancellationToken).ConfigureAwait(false);
			return true;
		}
		catch (Exception ex) when (ex is not OperationCanceledException)
		{
			Logger.LogError(ex);
			WalletSession.SetMaxBestHeight(CalculateSafestHeight());
			WalletSession.ReportInitializationFailure(ex);
			return false;
		}
	}
	private async Task PrepareStoresAsync(CancellationToken cancellationToken)
	{
		await TransactionStore.InitializeAsync(cancellationToken).ConfigureAwait(false);
		await FilterStore.InitializeAsync(CalculateSafestHeight(), cancellationToken).ConfigureAwait(false);
	}
	private async Task RecoverStorageAsync(CancellationToken cancellationToken)
	{
		await PrepareStoresAsync(cancellationToken).ConfigureAwait(false);
		await StartSynchronizationAsync(cancellationToken).ConfigureAwait(false);
	}
	private async Task StartSynchronizationAsync(CancellationToken cancellationToken)
	{
		using (await _networkInitializationLock.LockAsync(cancellationToken).ConfigureAwait(false))
		{
			if (_synchronizerStarted) { return; }
			await ConfigureSynchronizerAsync(_stoppingCts.Token).ConfigureAwait(false);
			_publicConnectionManager.Start(_stoppingCts.Token);
			_synchronizerStarted = true;
		}
	}

	private ChainHeight CalculateSafestHeight()
	{
		// Until setup chooses an account (or a legacy account's birthday is known), recovery must remain possible from the earliest supported block.
		if (!WalletSession.HasWallet() || (WalletSession.Snapshot.PublicMetadataRequiresAuthorization && WalletSession.GetBirthHeight() is null)) { return ChainHeight.Genesis; }
		var checkpointHeight = FilterCheckpoints.GetMostRecentCheckpoint(Network).Header.Height;
		var transactionHeight = TransactionStore.TryGetOldestKnownTransactionHeight(out var h)
			? h > Constants.ResyncHeightMargin
				? h - Constants.ResyncHeightMargin
				: h
			: checkpointHeight;
		var birthHeight = WalletSession.GetBirthHeight();
		var worstBestHeight = WalletSession.GetBestHeight();
		return (ChainHeight) Height.Min(checkpointHeight, ((ChainHeight?[]) [transactionHeight, birthHeight, worstBestHeight]).DropNulls());
	}

	public Task InitializeAsync(bool initializeSleepInhibitor, TerminateService terminateService, CancellationToken cancellationToken)
	{
		cancellationToken.ThrowIfCancellationRequested();
		lock (_initializationGate) { return _initializationTask ??= InitializeCoreAsync(initializeSleepInhibitor, terminateService, cancellationToken); }
	}
	private async Task InitializeCoreAsync(bool initializeSleepInhibitor, TerminateService terminateService, CancellationToken cancellationToken)
	{
		using CancellationTokenSource linkedCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, _stoppingCts.Token);
		CancellationToken linkedCtsToken = linkedCts.Token;

		ConfigureMagicalCryptoWalletUpdater(_stoppingCts.Token);
		ConfigureTorStatusChecker(_stoppingCts.Token);
		ConfigureExchangeRateUpdater(_stoppingCts.Token);
		ConfigureFeeRateUpdater(_stoppingCts.Token);

		// _stoppingCts may be disposed at this point, so do not forward the cancellation token here.
		using (await _initializationAsyncLock.LockAsync(linkedCtsToken))
		{
			Logger.LogTrace("Initialization started.");

			await StartRpcServerAsync(terminateService, linkedCtsToken).ConfigureAwait(false);
			var storage = InitializeBitcoinStoreAsync(linkedCtsToken);
			try { await StartTorProcessManagerAsync(linkedCtsToken).ConfigureAwait(false); }
			catch (Exception ex) when (ex is not OperationCanceledException) { Logger.LogWarning(ex); }
			if (await storage.ConfigureAwait(false)) { await StartSynchronizationAsync(linkedCtsToken).ConfigureAwait(false); }

			if (_disposeRequested)
			{
				return;
			}

			try
			{
				if (Config.TryGetCoordinatorUri(out var coordinatorUri))
				{

					if (initializeSleepInhibitor)
					{
						await CreateSleepInhibitorAsync().ConfigureAwait(false);
					}
				}

				await HostedServices.StartAllAsync(linkedCtsToken).ConfigureAwait(false);

			}
			finally
			{
				Logger.LogTrace("Initialization finished.");
			}
			_ticker.Change(TimeSpan.Zero, TimeSpan.FromSeconds(1));
		}
	}

	private async Task CreateSleepInhibitorAsync()
	{
		SleepInhibitor? sleepInhibitor = await SleepInhibitor.CreateAsync(HostedServices.Get<CoinJoinManager>()).ConfigureAwait(false);

		if (sleepInhibitor is not null)
		{
			HostedServices.Register<SleepInhibitor>(() => sleepInhibitor, "Sleep Inhibitor");
		}
		else
		{
			Logger.LogInfo("Sleep Inhibitor is not available on this platform.");
		}
	}

	private async Task StartRpcServerAsync(TerminateService terminateService, CancellationToken cancel)
	{
		// HttpListener doesn't support onion services as prefix and for that reason we have no alternative
		// other than using
		var prefixes = Config is { RpcOnionEnabled: true, JsonRpcServerEnabled: true } && Config.UseTor != TorMode.Disabled && !string.IsNullOrEmpty(Config.JsonRpcUser) && !string.IsNullOrEmpty(Config.JsonRpcPassword)
			? Config.JsonRpcServerPrefixes.Append($"http://+:38129/").ToArray()
			: Config.JsonRpcServerPrefixes;

		var jsonRpcServerConfig = new JsonRpcServerConfiguration(Config.JsonRpcServerEnabled, Config.JsonRpcUser, Config.JsonRpcPassword, prefixes, Config.Network);
		if (jsonRpcServerConfig.IsEnabled)
		{
			var magicalcryptowalletJsonRpcService = new MagicalCryptoWalletJsonRpcService(global: this);
			RpcServer = new JsonRpcServer(magicalcryptowalletJsonRpcService, jsonRpcServerConfig, terminateService);
			RpcServer.DisposeUsing(_disposables);
			try
			{
				await RpcServer.StartAsync(cancel).ConfigureAwait(false);
				RpcServer.DisposeUsing(_disposables);
			}
			catch (HttpListenerException e)
			{
				Logger.LogWarning($"Failed to start {nameof(JsonRpcServer)} with error: {e.Message}.");
				RpcServer = null;
			}
		}
	}

	private async Task StartTorProcessManagerAsync(CancellationToken cancellationToken)
	{
		if (Config.UseTor != TorMode.Disabled)
		{
			TorProcessManager processManager = new(TorSettings, EventBus);
			_torManager = new TorManager(TorSettings, processManager);
			_torManager.DisposeUsing(_asyncDisposables);
			await _torManager.StartAsync(attempts: 3, cancellationToken).ConfigureAwait(false);
			Logger.LogInfo($"{nameof(TorManager)} is initialized.");

			var (_, torControlClient) = await _torManager.WaitForNextAttemptAsync(cancellationToken).ConfigureAwait(false);
			if (Config is { JsonRpcServerEnabled: true, RpcOnionEnabled: true } && torControlClient is { } nonNullTorControlClient)
			{
				var anonymousAccessAllowed = string.IsNullOrEmpty(Config.JsonRpcUser) || string.IsNullOrEmpty(Config.JsonRpcPassword);
				if (!anonymousAccessAllowed)
				{
					var onionServiceId = await nonNullTorControlClient.CreateEphemeralOnionServiceAsync(80, 38129, cancellationToken).ConfigureAwait(false);
					OnionServiceUri = new Uri($"http://{onionServiceId}.onion");
					Logger.LogInfo($"RPC server listening on {OnionServiceUri}");
				}
				else
				{
					Logger.LogInfo("Anonymous access RPC server cannot be exposed as onion service.");
				}
			}

		}
	}

	private void ConfigureTorStatusChecker(CancellationToken cancellationToken)
	{
		if (Config.UseTor != TorMode.Disabled)
		{
			var torStatusHttpClient = PublicSourcesHttpClientFactory.CreateClient("long-live-torproject");
			var torStatusChecker = Spawn("TorStatusChecker",
				Periodically(
					TimeSpan.FromHours(1),
					Unit.Instance,
					TorStatusChecker.CreateChecker(torStatusHttpClient, EventBus)),
				cancellationToken);
			torStatusChecker.DisposeUsing(_disposables);
			EventBus.Subscribe<Tick>(_ => torStatusChecker.Post(new TorStatusChecker.CheckMessage()))
				.DisposeUsing(_disposables);
			torStatusChecker.Post(new TorStatusChecker.CheckMessage());
		}
	}

	private void RegisterCoinJoinComponents(Uri coordinatorUri)
	{
		var prisonForCoordinator = Path.Combine(DataDir, coordinatorUri.Host);
		_coinPrison = CoinPrison.CreateOrLoadFromFile(prisonForCoordinator);
		_coinPrison.DisposeUsing(_disposables);

		EventBus.Subscribe<WalletRelevantTransactionProcessed>(_ =>
		{
			if (WalletSession.GetWallet() is { } wallet) { _coinPrison.UpdateWallet(wallet); }
		}).DisposeUsing(_disposables);
		if (WalletSession.GetWallet() is { } configured) { _coinPrison.UpdateWallet(configured); }

		// Active protocol operations keep their retry budget; status polling uses a smaller budget.
		var coordinatorHttpClientConfig = new HttpClientHandlerConfiguration
		{
			MaxAttempts = 10,
			TimeBeforeRetryingAfterNetworkError = TimeSpan.FromSeconds(0.5),
			TimeBeforeRetryingAfterServerError = TimeSpan.FromSeconds(0.5),
			TimeBeforeRetryingAfterTooManyRequests = TimeSpan.FromSeconds(2)
		};
		var coordinatorHttpClientFactory = new CoordinatorHttpClientFactory(coordinatorUri, BuildHttpClientFactory(coordinatorHttpClientConfig));

		var statusHttpClientFactory = new CoordinatorHttpClientFactory(coordinatorUri,
			BuildHttpClientFactory(coordinatorHttpClientConfig with { MaxAttempts = 3 }));
		var wabiSabiStatusProvider = new WabiSabiHttpApiClient("satoshi-coordination", statusHttpClientFactory);
		var roundUpdater = Spawn("RoundUpdater",
			Service("WabiSabi Rounds Updater",
				EventDriven(
					new RoundsState(DateTime.UtcNow, RoundStateProvider.QueryFrequency, new Dictionary<uint256, RoundState>(), []),
					RoundStateUpdater.Create(wabiSabiStatusProvider))),
			_stoppingCts.Token);
		roundUpdater.DisposeUsing(_disposables);
		EventBus.Subscribe<Tick>(_ => roundUpdater.Post(new RoundUpdateMessage.UpdateMessage(DateTime.UtcNow)))
			.DisposeUsing(_disposables);

		Func<string, WabiSabiHttpApiClient> wabiSabiHttpClientFactory = (identity) => new WabiSabiHttpApiClient(identity, coordinatorHttpClientFactory);
		var coinJoinConfiguration = new CoinJoinConfiguration(Config.CoordinatorIdentifier, Config.MaxCoinjoinMiningFeeRate);
		HostedServices.Register<CoinJoinManager>(() => new CoinJoinManager(WalletSession, new RoundStateProvider(roundUpdater), wabiSabiHttpClientFactory, coinJoinConfiguration, _coinPrison, EventBus), "CoinJoin Manager");
	}

	private List<IBroadcaster> CreateBroadcasters(P2pNodeListProvider p2PNodeListProvider, MempoolService mempoolService)
	{
		List<IBroadcaster> result =
		[
			new NetworkBroadcaster(mempoolService, p2PNodeListProvider, Network.MinBroadcastNodes, PrepareBroadcastPeersAsync)
		];

		if (Network != Network.RegTest)
		{
			var external = ExternalTransactionBroadcaster.GetSortedBroadcasters(Config.ExternalTransactionBroadcaster, Network)
				.Select(info => new ExternalTransactionBroadcaster(info, ExternalSourcesHttpClientFactory));
			result.AddRange(external);
		}

		return result;
	}

	private async Task PrepareBroadcastPeersAsync(CancellationToken cancellationToken)
	{
		_p2pConnectionManager.Start(_stoppingCts.Token);
		using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
		using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, _stoppingCts.Token, timeout.Token);
		try
		{
			// Two other peers must announce propagation; peers we announce to need not echo our inventory.
			var target = Network.MinBroadcastNodes == 1 ? 1 : Network.MinBroadcastNodes + 2;
			while (_p2pConnectionManager.Nodes.Length < target)
			{
				await Task.Delay(TimeSpan.FromMilliseconds(100), linked.Token).ConfigureAwait(false);
			}
		}
		catch (OperationCanceledException) when (timeout.IsCancellationRequested && !cancellationToken.IsCancellationRequested && !_stoppingCts.IsCancellationRequested)
		{
			Logger.LogInfo("Broadcast peer discovery timed out; trying available peers and transaction broadcast services.");
		}
	}

	public ImmutableArray<Node> GetNodes() => ReferenceEquals(_p2pConnectionManager, _publicConnectionManager)
		? _p2pConnectionManager.Nodes
		: _p2pConnectionManager.Nodes.AddRange(_publicConnectionManager.Nodes);
	public uint GetBlockHeadersTipHeight() => (uint)(_blockHeaders.Tip?.Height ?? 0);
	public int GetPeerCount() => GetNodes().Length;

	public async Task DisposeAsync()
	{
		// Dispose method may be called just once.
		if (!_disposeRequested)
		{
			_disposeRequested = true;
			if (HostedServices.GetOrDefault<CoinJoinManager>() is { IsStarted: true } manager)
			{
				using var timeout = new CancellationTokenSource(TimeSpan.FromMinutes(6));
				try { await manager.StopAsync(timeout.Token).ConfigureAwait(false); }
				catch (Exception ex) { Logger.LogWarning(ex); }
			}
			await _stoppingCts.CancelAsync().ConfigureAwait(false);
		}
		else
		{
			return;
		}

		using (await _initializationAsyncLock.LockAsync())
		{
			Logger.LogWarning("Process is exiting.");

			try
			{


				if (Network != Network.RegTest && _blockHeaders.Tip is not null)
				{
					var p2PDataDir = GetBitcoinP2PNetworkDirectory();
					var blockHeadersFilePath = Path.Combine(p2PDataDir, $"BlockHeaders{Network}.dat");
					File.SafelyWriteAllBytes(blockHeadersFilePath, _blockHeaders.ToBytes());
					Logger.LogInfo("Block headers saved.");
				}

				if (RpcServer is { } rpcServer)
				{
					using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(21));
					try { await rpcServer.StopAsync(cts.Token).ConfigureAwait(false); }
					catch (Exception ex) { Logger.LogWarning(ex); }
					Logger.LogInfo($"{nameof(RpcServer)} is stopped.");
				}

				if (HostedServices is { } backgroundServices)
				{
					using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(21));
					try { await backgroundServices.StopAllAsync(cts.Token).ConfigureAwait(false); }
					catch (Exception ex) { Logger.LogWarning(ex); }
					Logger.LogInfo("Stopped background services.");
				}

				try
				{
					using var dequeueCts = new CancellationTokenSource(TimeSpan.FromMinutes(6));
					await WalletSession.StopAsync(dequeueCts.Token).ConfigureAwait(false);
					Logger.LogInfo($"{nameof(WalletSession)} is stopped.");
				}
				catch (Exception ex)
				{
					Logger.LogError($"Error during {nameof(WalletSession.StopAsync)}: {ex}");
				}

				if (_torManager is not null)
				{
					using CancellationTokenSource cts = new(TimeSpan.FromSeconds(5));

					var torControlClient =
						Result<(CancellationToken, TorControlClient), Exception>
						.Catch(async () => await _torManager.WaitForNextAttemptAsync(cts.Token).ConfigureAwait(false))
						.Map(x => x.Result.Item2)
						.AsNullable();

					if (OnionServiceUri is { } nonNullOnionServiceUri && torControlClient is { } nonNullTorControlClient)
					{
						try
						{
							var isDestroyedSuccessfully = await nonNullTorControlClient
								.DestroyOnionServiceAsync(nonNullOnionServiceUri.Host, cts.Token).ConfigureAwait(false);
							if (!isDestroyedSuccessfully)
							{
								Logger.LogInfo($"Onion service '{nonNullOnionServiceUri.Host}' failed to be destroyed.");
							}
						}
						catch (OperationCanceledException)
						{
							Logger.LogInfo($"'{nonNullOnionServiceUri.Host}' failed to be stopped in allotted time.");
						}
					}

					Logger.LogInfo("TorManager is stopped.");
				}

				_disposables.Dispose();
				await _asyncDisposables.DisposeAsync().ConfigureAwait(false);
			}
			catch (Exception ex)
			{
				Logger.LogWarning(ex);
			}
			finally
			{
				Logger.LogTrace("Dispose finished.");
			}
		}
	}
}
