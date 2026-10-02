using System.Collections.Generic;
using System.IO;
using System.Net.Http;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.Blocks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionBroadcasting;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Services.Terminate;
using MagicalCryptoWallet.Stores;
using MagicalCryptoWallet.Tor;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent;

public class Services : IServices
{
	// Temporary solution. It should be removed
	public static Services Instance { get; private set; } = null!;

	private readonly Global _global;
	private readonly TorSettings _torSettings;
	private readonly FilterStore _filterStore;
	private readonly FilterHeaderChain _filterHeaders;
	private readonly AllTransactionStore _transactionStore;
	private readonly IHttpClientFactory _httpClientFactory;
	private readonly TransactionBroadcaster _transactionBroadcaster;
	private readonly HostedServices _hostedServices;
	private readonly TerminateService _terminateService;
	private readonly StatusContainer _status;

	private Services(Global global, UiConfig uiConfig, TerminateService terminateService)
	{
		Guard.NotNull(nameof(global.DataDir), global.DataDir);
		Guard.NotNull(nameof(global.TorSettings), global.TorSettings);
		Guard.NotNull(nameof(global.FilterStore), global.FilterStore);
		Guard.NotNull(nameof(global.FilterHeaders), global.FilterHeaders);
		Guard.NotNull(nameof(global.TransactionStore), global.TransactionStore);
		Guard.NotNull(nameof(global.ExternalSourcesHttpClientFactory), global.ExternalSourcesHttpClientFactory);
		Guard.NotNull(nameof(global.Config), global.Config);
		Guard.NotNull(nameof(global.WalletSession), global.WalletSession);
		Guard.NotNull(nameof(global.TransactionBroadcaster), global.TransactionBroadcaster);
		Guard.NotNull(nameof(global.HostedServices), global.HostedServices);
		Guard.NotNull(nameof(uiConfig), uiConfig);
		Guard.NotNull(nameof(terminateService), terminateService);

		_global = global;
		_torSettings = global.TorSettings;
		_filterStore = global.FilterStore;
		_filterHeaders = global.FilterHeaders;
		_transactionStore = global.TransactionStore;
		_httpClientFactory = global.ExternalSourcesHttpClientFactory;
		_transactionBroadcaster = global.TransactionBroadcaster;
		_hostedServices = global.HostedServices;
		_terminateService = terminateService;
		_status = global.Status;
		Scheme = global.Scheme;
		DataDir = global.DataDir;
		PersistentConfig = global.Config.PersistentConfig;
		WalletSession = global.WalletSession;
		UiConfig = uiConfig;
		Config = global.Config;
		EventBus = global.EventBus;
	}

	public string DataDir { get; }
	public string PersistentConfigFilePath => Path.Combine(DataDir, PersistentConfig.GetConfigFileName());
	public PersistentConfig PersistentConfig { get; }
	public WalletSession WalletSession { get; }
	public UiConfig UiConfig { get; }
	public Config Config { get; }
	public EventBus EventBus { get; }
	public Client.Scheme Scheme { get; }

	// Chain info
	public uint GetTipHeight() => _filterHeaders.TipHeight;
	public uint GetServerTipHeight() => _filterHeaders.ServerTipHeight;
	public int GetHashesLeft() => _filterHeaders.HashesLeft;
	public SmartHeader? GetTip() => _filterHeaders.Tip;
	public uint GetBlockHeadersTipHeight() => _global.GetBlockHeadersTipHeight();
	public int GetPeerCount() => _global.GetPeerCount();

	// Filters info
	public uint? GetMinimumBlockHeight() => _filterStore.GetMinimumBlockHeight();

	// Transactions info
	public IEnumerable<LabelsArray> GetTransactionLabels() => _transactionStore.GetLabels();

	// WalletSession info
	public Network GetNetwork() => WalletSession.Network;






	// Tor info
	public string GetTorLogFilePath() => _torSettings.LogFilePath;
	public TorMode GetUseTor() => Config.UseTor;

	// ExchangeRate info
	public decimal GetUsdExchangeRate() => _status.UsdExchangeRate;

	// UI Config
	public bool GetHideOnClose() => UiConfig.HideOnClose;
	public double? GetWindowWidth() => UiConfig.WindowWidth;
	public double? GetWindowHeight() => UiConfig.WindowHeight;
	public void SetWindowWidth(double? width) => UiConfig.WindowWidth = width;
	public void SetWindowHeight(double? height) => UiConfig.WindowHeight = height;
	public bool GetPrivacyMode() => UiConfig.PrivacyMode;
	public bool GetAutocopy() => UiConfig.Autocopy;
	public bool GetAutoPaste() => UiConfig.AutoPaste;
	public bool GetSendAmountConversionReversed() => UiConfig.SendAmountConversionReversed;
	public void SetSendAmountConversionReversed(bool value) => UiConfig.SendAmountConversionReversed = value;

	// Temporary solution
	public T? GetHostedService<T>() where T : class, Microsoft.Extensions.Hosting.IHostedService => _hostedServices.GetOrDefault<T>();

	// Transaction
	public async Task SendTransactionAsync(SmartTransaction transaction)
	{
		WalletSession.EnsureReady();
		var manager = GetHostedService<MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager.CoinJoinManager>();
		manager?.WalletEnteredSendWorkflow();
		try
		{
			if (manager is not null) { await manager.WalletEnteredSendingAsync().ConfigureAwait(false); }
			WalletSession.EnsureReady();
			await _transactionBroadcaster.SendTransactionAsync(transaction).ConfigureAwait(false);
		}
		finally { manager?.WalletLeftSendWorkflow(); }
	}

	// HttpClientFactory wrapper functions
	public HttpClient CreateHttpClient(string name) => _httpClientFactory.CreateClient(name);

	// TerminateService wrapper functions
	public bool IsForcefulTerminationRequested() => _terminateService.ForcefulTerminationRequestedTask.IsCompletedSuccessfully;

	public static Services Create(Global global, UiConfig uiConfig, TerminateService terminateService)
	{
		Instance = new Services(global, uiConfig, terminateService);
		return Instance;
	}
}
