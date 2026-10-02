using Microsoft.Extensions.Hosting;
using NBitcoin;
using System.Collections.Generic;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Backend.Models;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.Blocks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.Mempool;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Blockchain.TransactionProcessing;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.FeeRateEstimation;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Stores;
using MagicalCryptoWallet.Userfacing;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Client.Batching;
using static MagicalCryptoWallet.Logging.LoggerTools;

namespace MagicalCryptoWallet.Wallets;

public delegate Wallet WalletFactory(KeyManager keyManager);

public class Wallet : BackgroundService
{
	private readonly ComposedDisposable _disposables = new();
	private readonly Lock _mempoolGate = new();
	private bool _stopping;

	public static WalletFactory CreateFactory(
		Network network, FilterStore filterStore, AllTransactionStore transactionStore, FilterHeaderChain filterHeaderChain,
		MempoolService mempoolService, ServiceConfiguration serviceConfiguration, BlockProvider blockProvider,
		EventBus eventBus, CpfpInfoProvider cpfpInfoProvider) =>
		keyManager => new Wallet(network, keyManager, filterStore, transactionStore, filterHeaderChain, blockProvider, mempoolService, serviceConfiguration, cpfpInfoProvider, eventBus);

	private Wallet(
		Network network,
		KeyManager keyManager,
		FilterStore filterStore,
		AllTransactionStore transactionStore,
		FilterHeaderChain filterHeaderChain,
		BlockProvider blockProvider,
		MempoolService mempoolService,
		ServiceConfiguration serviceConfiguration,
		CpfpInfoProvider cpfpInfoProvider,
		EventBus eventBus)
	{
		Network = network;
		KeyManager = keyManager;
		ServiceConfiguration = serviceConfiguration;
		CpfpInfoProvider = cpfpInfoProvider;
		DestinationProvider = new InternalDestinationProvider(KeyManager);
		_filterStore = filterStore;
		TransactionStore = transactionStore;
		FilterHeaderChain = filterHeaderChain;

		TransactionProcessor = new TransactionProcessor(TransactionStore, mempoolService, keyManager, ServiceConfiguration.DustThreshold, eventBus);
		WalletFilterProcessor = new WalletFilterProcessor(keyManager, TransactionStore, _filterStore, FilterHeaderChain, TransactionProcessor, blockProvider, eventBus);
		Coins = TransactionProcessor.Coins;
		BatchedPayments = new PaymentBatch();
		OutputProvider = new PaymentAwareOutputProvider(DestinationProvider, BatchedPayments, RandomnessProviders.Secure);
		_eventBus = eventBus;

		_eventBus.Subscribe<MiningFeeRatesChanged>(e => FeeRateEstimations = e.AllFeeEstimate)
			.DisposeUsing(_disposables);
		_eventBus.Subscribe<WalletRelevantTransactionProcessed>(e =>
		{
			WalletRelevantTransactionProcessed(e.Result);
		})
			.DisposeUsing(_disposables);
		_eventBus.Subscribe<NewTransactionInMempool>(e => Mempool_TransactionReceived(e.Transaction))
			.DisposeUsing(_disposables);
	}

	private readonly EventBus _eventBus;
	private readonly FilterStore _filterStore;
	public AllTransactionStore TransactionStore { get; }
	public FilterHeaderChain FilterHeaderChain { get; }

	public KeyManager KeyManager { get; }
	public ServiceConfiguration ServiceConfiguration { get; }
	public FeeRateEstimations? FeeRateEstimations { get; private set; }

	public CoinsRegistry Coins { get; }



	public Network Network { get; }
	public TransactionProcessor TransactionProcessor { get; }

	public CpfpInfoProvider CpfpInfoProvider { get; }
	public WalletFilterProcessor WalletFilterProcessor { get; }

	public Task InitialSynchronizationFinished => WalletFilterProcessor.InitialSynchronizationFinished;
	public bool HasCachedData { get; private set; }

	public IDestinationProvider DestinationProvider { get; }

	public OutputProvider OutputProvider { get; }
	public PaymentBatch BatchedPayments { get; }

	public int AnonScoreTarget => Constants.AnonymityScoreTarget;

	public Money PlebStopThreshold => KeyManager.PlebStopThreshold;

	public ICoinsView GetAllCoins() => Coins.AsAllCoinsView();

	public bool IsWalletPrivate() => GetPrivacyPercentage() >= 100;

	public IEnumerable<SmartCoin> GetCoinjoinCoinCandidates() => Coins;

	/// <summary>
	/// Get all the transactions associated to the wallet ordered by blockchain.
	/// </summary>
	public IEnumerable<SmartTransaction> GetTransactions()
	{
		var walletTransactions = new HashSet<SmartTransaction>();

		foreach (SmartCoin coin in GetAllCoins())
		{
			walletTransactions.Add(coin.Transaction);
			if (coin.SpenderTransaction is not null)
			{
				walletTransactions.Add(coin.SpenderTransaction);
			}
		}

		return walletTransactions.OrderByBlockchain().ToList();
	}

	/// <summary>
	/// Get all wallet transactions along with corresponding amounts ordered by blockchain.
	/// </summary>
	/// <param name="sortForUi"><c>true</c> to sort by "first seen", "height", and "block index", <c>false</c> to sort by "height", "block index", and "first seen".</param>
	/// <remarks>Transaction amount specifies how it affected your final wallet balance (spend some bitcoin, received some bitcoin, or no change).</remarks>
	public async Task<List<TransactionSummary>> BuildHistorySummaryAsync(bool sortForUi = false, CancellationToken cancellationToken = default)
	{
		var cpfpInfos = await CpfpInfoProvider.GetCachedCpfpInfoAsync(cancellationToken).ConfigureAwait(false);

		Dictionary<uint256, TransactionSummary> mapByTxid = new();

		foreach (SmartCoin coin in GetAllCoins())
		{
			if (mapByTxid.TryGetValue(coin.TransactionId, out TransactionSummary? found)) // If found then update.
			{
				found.Amount += coin.Amount;
			}
			else
			{
				FeeRate? effectiveFeeRate = null;
				if (cpfpInfos.FirstOrDefault(x => x.Transaction == coin.Transaction) is { } cachedCpfpInfo)
				{
					effectiveFeeRate = new FeeRate(cachedCpfpInfo.CpfpInfo.EffectiveFeePerVSize);
				}

				mapByTxid.Add(coin.TransactionId, new TransactionSummary(coin.Transaction, coin.Amount, effectiveFeeRate));
			}

			if (coin.SpenderTransaction is { } spenderTransaction)
			{
				var spenderTxId = spenderTransaction.GetHash();

				if (mapByTxid.TryGetValue(spenderTxId, out TransactionSummary? foundSpenderCoin)) // If found then update.
				{
					foundSpenderCoin.Amount -= coin.Amount;
				}
				else
				{
					FeeRate? effectiveFeeRate = null;
					if (cpfpInfos.FirstOrDefault(x => x.Transaction == coin.Transaction) is { } cachedCpfpInfo)
					{
						effectiveFeeRate = new FeeRate(cachedCpfpInfo.CpfpInfo.EffectiveFeePerVSize);
					}

					mapByTxid.Add(spenderTxId, new TransactionSummary(spenderTransaction, Money.Zero - coin.Amount, effectiveFeeRate));
				}
			}
		}

		return sortForUi
			? mapByTxid.Values.OrderBy(x => x.FirstSeen).ThenBy(x => x.Height).ThenBy(x => x.BlockIndex).ToList()
			: mapByTxid.Values.OrderByBlockchain().ToList();
	}

	public HdPubKey GetNextReceiveAddress(IEnumerable<string> destinationLabels, ScriptPubKeyType scriptPubKeyType)
	{
		return KeyManager.GetNextReceiveKey(new LabelsArray(destinationLabels), scriptPubKeyType);
	}

	public int GetPrivacyPercentage()
	{
		var coins = Coins.ToArray();
		var total = coins.Sum(x => x.Amount.Satoshi);
		var privateAmount = coins.Where(x => x.IsPrivate(Constants.AnonymityScoreTarget)).Sum(x => x.Amount.Satoshi);
		return total == 0 ? 0 : (int)(privateAmount * 100m / total);
	}

	public void InitializeLocalState()
	{
		if (HasCachedData) { return; }
		KeyManager.GetKeys();
		TransactionProcessor.Process(TransactionStore.ConfirmedStore.GetTransactions(), isHistoricalReplay: true);
		TransactionProcessor.Process(TransactionStore.MempoolStore.GetTransactions(), isHistoricalReplay: true);
		HasCachedData = true;
	}

	/// <inheritdoc/>
	public override async Task StartAsync(CancellationToken cancellationToken)
	{
		InitializeLocalState();
		await WalletFilterProcessor.StartAsync(cancellationToken).ConfigureAwait(false);
		Logger.LogTrace(FormatLog("Wallet filter processor is started.", this));

		await LoadWalletStateAsync(cancellationToken).ConfigureAwait(false);
		Logger.LogTrace(FormatLog("State is loaded.", this));

		LoadDummyMempool();

		await base.StartAsync(cancellationToken).ConfigureAwait(false);

	}

	/// <inheritdoc />
	protected override async Task ExecuteAsync(CancellationToken stoppingToken)
	{
		Logger.LogInfo(FormatLog("is fully synchronized.", this));
	}

	public string AddCoinJoinPayment(IDestination destination, Money amount)
	{
		var paymentId = BatchedPayments.AddPayment(destination, amount);
		_eventBus.Publish(new PaymentBatchChanged(BatchedPayments.GetPayments()));
		return paymentId.ToString();
	}

	public void CancelCoinJoinPayment(Guid paymentId)
	{
		BatchedPayments.AbortPayment(paymentId);
		_eventBus.Publish(new PaymentBatchChanged(BatchedPayments.GetPayments()));
	}

	/// <inheritdoc/>
	public override async Task StopAsync(CancellationToken cancel)
	{
		StopSubscriptions();
		await base.StopAsync(cancel).ConfigureAwait(false);
		await WalletFilterProcessor.StopAsync(cancel).ConfigureAwait(false);
	}

	public override void Dispose()
	{
		StopSubscriptions();
		WalletFilterProcessor.Dispose();
		base.Dispose();
	}

	private void StopSubscriptions()
	{
		lock (_mempoolGate)
		{
			_stopping = true;
			_disposables.Dispose();
		}
	}

	private void WalletRelevantTransactionProcessed(ProcessedResult e)
	{
		try
		{
			if (e.Transaction.CanBeSpeedUpUsingCpfp())
			{
				CpfpInfoProvider.ScheduleRequest(e.Transaction);
			}

			// Check if this transaction resolves any uncertain payments in coinjoins
			// If the transaction has outputs matching our pending payments, mark them as finished.
			if (BatchedPayments.AreThereUncertainPayments && BatchedPayments.TryResolvePaymentsWithTransaction(e.Transaction))
			{
				_eventBus.Publish(new PaymentBatchChanged(BatchedPayments.GetPayments()));
			}
		}
		catch (Exception ex)
		{
			Logger.LogError(FormatLog(ex.ToString(), this));
		}
	}

	private void Mempool_TransactionReceived(SmartTransaction tx)
	{
		lock (_mempoolGate)
		{
			if (_stopping) { return; }
			try
			{
				if (!TransactionProcessor.IsAware(tx.GetHash()))
				{
					TransactionProcessor.Process(tx);
				}
			}
			catch (Exception ex)
			{
				Logger.LogWarning(FormatLog(ex.ToString(), this));
			}
		}
	}

	private async Task LoadWalletStateAsync(CancellationToken cancellationToken)
	{
		// Make sure that the keys are asserted in case of an empty HdPubKeys array.
		KeyManager.GetKeys();

		InitializeLocalState();

		Logger.LogTrace(FormatLog("Waiting for initial synchronization to finish.", this));
		await WalletFilterProcessor.InitialSynchronizationFinished.WaitAsync(cancellationToken).ConfigureAwait(false);
	}

	private void LoadDummyMempool()
	{
		if (TransactionStore.MempoolStore.IsEmpty())
		{
			return;
		}

		// Only clean the mempool if we're fully synchronized.
		if (FilterHeaderChain.HashesLeft == 0)
		{
			var txsToProcess = new List<SmartTransaction>();
			foreach (var tx in TransactionStore.MempoolStore.GetTransactions())
			{
				var txid = tx.GetHash();
				if (DateTimeOffset.UtcNow - tx.FirstSeen < TimeSpan.FromDays(ServiceConfiguration.DropUnconfirmedTransactionsAfterDays))
				{
					txsToProcess.Add(tx);
				}
				else
				{
					if (TransactionStore.MempoolStore.TryRemove(txid, out _))
					{
						Logger.LogInfo(FormatLog($"Transaction {txid} dropped after {ServiceConfiguration.DropUnconfirmedTransactionsAfterDays} days being unconfirmed.", this));
					}
				}
			}

			TransactionProcessor.Process(txsToProcess, isHistoricalReplay: true);
		}
		else
		{
			TransactionProcessor.Process(TransactionStore.MempoolStore.GetTransactions(), isHistoricalReplay: true);
		}
	}

	public void UpdateUsedHdPubKeysLabels(Dictionary<HdPubKey, LabelsArray> hdPubKeysWithLabels)
	{
		if (hdPubKeysWithLabels.Count == 0)
		{
			return;
		}

		foreach (var item in hdPubKeysWithLabels)
		{
			item.Key.SetLabel(item.Value);
		}

		KeyManager.ToFile();
	}
}
