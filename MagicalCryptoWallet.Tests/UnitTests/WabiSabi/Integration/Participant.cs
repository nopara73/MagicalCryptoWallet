using System.Collections.Concurrent;
using System.Diagnostics;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.BitcoinRpc;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Tests.UnitTests.Services;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Client.RoundStateAwaiters;
using MagicalCryptoWallet.WabiSabi.Coordinator.PostRequests;

namespace MagicalCryptoWallet.Tests.UnitTests.WabiSabi.Integration;

internal class Participant
{
	public Participant(string name, IRPCClient rpc, Func<string, IWabiSabiApiRequestHandler> apiClientFactory)
	{
		HttpClientFactory = apiClientFactory;

		Wallet = new TestWallet(name, rpc);
	}

	private TestWallet Wallet { get; }
	public Func<string, IWabiSabiApiRequestHandler> HttpClientFactory { get; }
	private SmartTransaction? SplitTransaction { get; set; }
	public TestableCoinJoinClient? ActiveClient { get; private set; }
	public ConcurrentQueue<string> Progress { get; } = new();

	public async Task GenerateSourceCoinAsync(CancellationToken cancellationToken)
	{
		await Wallet.GenerateAsync(1, cancellationToken).ConfigureAwait(false);
	}

	public async Task GenerateCoinsAsync(int numberOfCoins, int seed, CancellationToken cancellationToken)
	{
		var feeRate = new FeeRate(4.0m);
		var (splitTx, spendingCoin) = Wallet.CreateTemplateTransaction();
		var availableAmount = spendingCoin.EffectiveValue(feeRate);

		var rnd = new Random(seed);
		double NextNotTooSmall() => 0.00001 + (rnd.NextDouble() * 0.99999);
		var sampling = Enumerable
			.Range(0, numberOfCoins - 1)
			.Select(_ => NextNotTooSmall())
			.Prepend(0)
			.Prepend(1)
			.OrderBy(x => x)
			.ToArray();

		var amounts = sampling
			.Zip(sampling.Skip(1), (x, y) => y - x)
			.Select(x => x * availableAmount.Satoshi)
			.Select(x => Money.Satoshis((long)x));

		foreach (var amount in amounts)
		{
			var outputAddress = Wallet.CreateNewAddress();
			var effectiveOutputValue = amount - feeRate.GetFee(outputAddress.ScriptPubKey.EstimateOutputVsize());
			splitTx.Outputs.Add(new TxOut(effectiveOutputValue, Wallet.CreateNewAddress()));
		}
		await Wallet.SendRawTransactionAsync(Wallet.SignTransaction(splitTx), cancellationToken).ConfigureAwait(false);
		SplitTransaction = new SmartTransaction(splitTx, new Height.ChainHeight(1));
	}

	public async Task<CoinJoinResult> StartParticipatingAsync(CancellationToken cancellationToken)
	{
		if (SplitTransaction is null)
		{
			throw new InvalidOperationException($"{nameof(GenerateCoinsAsync)} has to be called first.");
		}

		var apiClient = HttpClientFactory;
		using var roundStateUpdater = RoundStateUpdaterForTesting.Create(apiClient("satoshi"));
		var roundStateProvider = new RoundStateProvider(roundStateUpdater);

		var outputProvider = new OutputProvider(Wallet, RandomnessProviders.Insecure);
		var coinJoinClient = WabiSabiFactory.CreateTestCoinJoinClient(HttpClientFactory, Wallet, outputProvider, roundStateProvider);
		ActiveClient = (TestableCoinJoinClient)coinJoinClient;
		var elapsed = Stopwatch.StartNew();
		coinJoinClient.CoinJoinClientProgress += (_, progress) =>
			Progress.Enqueue($"{elapsed.Elapsed.TotalSeconds:F3}s {progress.GetType().Name}");

		static HdPubKey CreateHdPubKey(ExtPubKey extPubKey)
		{
			var hdPubKey = new HdPubKey(extPubKey.PubKey, KeyPath.Parse($"m/84'/0/0/0/{extPubKey.Child}"), LabelsArray.Empty, KeyState.Clean);
			hdPubKey.SetAnonymitySet(1); // bug if not settled
			return hdPubKey;
		}

		var smartCoins = SplitTransaction.Transaction.Outputs.AsIndexedOutputs()
			.Select(x => (IndexedTxOut: x, HdPubKey: Wallet.GetExtPubKey(x.TxOut.ScriptPubKey)))
			.Select(x => new SmartCoin(SplitTransaction, x.IndexedTxOut.N, CreateHdPubKey(x.HdPubKey)))
			.ToList();

		// Run the coinjoin client task.
		var result = await coinJoinClient.StartCoinJoinAsync(() => smartCoins, cancellationToken).ConfigureAwait(false);

		return result;
	}
}
