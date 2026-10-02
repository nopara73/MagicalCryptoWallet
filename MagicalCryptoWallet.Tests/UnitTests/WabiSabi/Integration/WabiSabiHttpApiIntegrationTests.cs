using Microsoft.Extensions.DependencyInjection;
using NBitcoin;
using System.Collections.Concurrent;
using System.Collections.Immutable;
using System.IO;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.BitcoinRpc;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Crypto;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Tests.UnitTests.Mocks;
using MagicalCryptoWallet.Tests.UnitTests.Services;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Client.RoundStateAwaiters;
using MagicalCryptoWallet.WabiSabi.Coordinator;
using MagicalCryptoWallet.WabiSabi.Coordinator.Models;
using MagicalCryptoWallet.WabiSabi.Coordinator.PostRequests;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.WabiSabi.Coordinator.Statistics;
using MagicalCryptoWallet.WabiSabi.Models;
using MagicalCryptoWallet.WabiSabi.Models.MultipartyTransaction;
using Xunit;
using static MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client.CoinJoinClient;

namespace MagicalCryptoWallet.Tests.UnitTests.WabiSabi.Integration;

/// <seealso cref="XunitConfiguration.SerialCollectionDefinition"/>
[Collection("Serial unit tests collection")]
public class WabiSabiHttpApiIntegrationTests : IClassFixture<WabiSabiApiApplicationFactory<Startup>>
{
	private readonly WabiSabiApiApplicationFactory<Startup> _apiApplicationFactory;
	private readonly ITestOutputHelper _output;

	public WabiSabiHttpApiIntegrationTests(WabiSabiApiApplicationFactory<Startup> apiApplicationFactory, ITestOutputHelper output)
	{
		_apiApplicationFactory = apiApplicationFactory;
		_output = output;
	}

	[Fact]
	public async Task RegisterSpentOrInNonExistentCoinAsync()
	{
		var httpClient = _apiApplicationFactory.CreateClient();
		await Task.Delay(100);
		var apiClient = await _apiApplicationFactory.CreateArenaClientAsync(httpClient);
		var rounds = (await apiClient.GetStatusAsync(RoundStateRequest.Empty, CancellationToken.None)).RoundStates;
		var round = rounds.First(x => x.CoinjoinState is ConstructionState);

		// If an output is not in the utxo dataset then it is not unspent, this
		// means that the output is spent or simply doesn't even exist.
		var nonExistingOutPoint = new OutPoint();
		using var signingKey = new Key();
		var ownershipProof = WabiSabiFactory.CreateOwnershipProof(signingKey, round.Id);

		var ex = await Assert.ThrowsAsync<WabiSabiProtocolException>(async () =>
		   await apiClient.RegisterInputAsync(round.Id, nonExistingOutPoint, ownershipProof, CancellationToken.None));

		Assert.Equal(WabiSabiProtocolErrorCode.InputSpent, ex.ErrorCode);
	}

	[Fact]
	public async Task RegisterBannedCoinAsync()
	{
		using CancellationTokenSource timeoutCts = new(TimeSpan.FromMinutes(2));

		using var signingKey = new Key();
		var coin = WabiSabiFactory.CreateCoin(signingKey);
		var bannedOutPoint = coin.Outpoint;

		var httpClient = _apiApplicationFactory.WithWebHostBuilder(builder =>
			builder.ConfigureServices(services =>
			{
				var rpc = BitcoinFactory.GetMockMinimalRpc();

				// Make the coordinator believe that the coins are real and
				// that they exist in the blockchain with many confirmations.
				rpc.OnGetTxOutAsync = (_, _, _) => new()
				{
					Confirmations = 101,
					IsCoinBase = false,
					ScriptPubKeyType = "witness_v0_keyhash",
					TxOut = coin.TxOut
				};
				services.AddSingleton<IRPCClient>(s => rpc);

				var prison = WabiSabiFactory.CreatePrison();
				prison.FailedVerification(bannedOutPoint, uint256.One);
				services.AddSingleton(_ => prison);
			})).CreateClient();

		await Task.Delay(100);
		var apiClient = await _apiApplicationFactory.CreateArenaClientAsync(httpClient);
		var rounds = (await apiClient.GetStatusAsync(RoundStateRequest.Empty, timeoutCts.Token)).RoundStates;
		var round = rounds.First(x => x.CoinjoinState is ConstructionState);

		// If an output is not in the utxo dataset then it is not unspent, this
		// means that the output is spent or simply doesn't even exist.
		var ownershipProof = WabiSabiFactory.CreateOwnershipProof(signingKey, round.Id);

		var ex = await Assert.ThrowsAsync<WabiSabiProtocolException>(async () =>
			await apiClient.RegisterInputAsync(round.Id, bannedOutPoint, ownershipProof, timeoutCts.Token));

		Assert.Equal(WabiSabiProtocolErrorCode.InputBanned, ex.ErrorCode);
		var inputBannedData = Assert.IsType<InputBannedExceptionData>(ex.ExceptionData);
		Assert.True(inputBannedData.BannedUntil > DateTimeOffset.UtcNow);
	}

	[Fact]
	public async Task UndersizedRoundsNeverRegisterOrBroadcastAsync()
	{
		var keys = KeyManager.CreateNew(out _, "", Network.Main);
		var coins = GenerateSmartCoins(keys, [10_000_000, 20_000_000], 2);
		bool broadcast = false;
		using var http = _apiApplicationFactory.WithWebHostBuilder(builder => builder
			.AddMockRpcClient(coins, rpc => rpc.OnSendRawTransactionAsync = tx => { broadcast = true; return tx.GetHash(); })
			.ConfigureServices(services => services.AddSingleton(_ => new WabiSabiConfig { MaxInputCountByRound = 20 }))).CreateClient();
		var api = _apiApplicationFactory.CreateWabiSabiHttpApiClient(http);
		using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2));
		using var updater = RoundStateUpdaterForTesting.Create(api, timeout.Token);
		var provider = new RoundStateProvider(updater);
		var client = WabiSabiFactory.CreateTestCoinJoinClient(_ => api, keys, provider);
		await Assert.ThrowsAnyAsync<OperationCanceledException>(() => client.StartCoinJoinAsync(() => coins, timeout.Token));
		Assert.False(broadcast);
	}

	[Fact]
	public async Task FailToRegisterOutputsCoinJoinTestAsync()
	{
		using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(90));
		var keys = Enumerable.Range(0, 3).Select(i => KeyManager.CreateNew(out _, "", Network.Main)).ToArray();
		var coins = keys.Select(key => GenerateSmartCoins(key, Enumerable.Repeat(10_000_000L, 7).ToArray(), 7)).ToArray();
		foreach (var key in keys)
		{
			key.AssertLockedInternalKeysIndexedAndPersist(21, false);
			key.AssertLockedInternalKeysIndexedAndPersist(21, true);
		}
		var usedScripts = keys.SelectMany(key => key.GetNextCoinJoinKeys())
			.SelectMany(key => new[] { key.PubKey.GetScriptPubKey(ScriptPubKeyType.Segwit), key.PubKey.GetScriptPubKey(ScriptPubKeyType.TaprootBIP86) }).ToImmutableArray();
		bool broadcast = false;
		using var http = _apiApplicationFactory.WithWebHostBuilder(builder => builder
			.AddMockRpcClient(coins.SelectMany(x => x).ToImmutableList(), rpc => rpc.OnSendRawTransactionAsync = tx => { broadcast = true; return tx.GetHash(); })
			.ConfigureServices(services =>
			{
				services.AddSingleton(_ => new WabiSabiConfig
				{
					MaxInputCountByRound = 21, MinInputCountByRoundMultiplier = 1,
					StandardInputRegistrationTimeout = TimeSpan.FromSeconds(20),
					ConnectionConfirmationTimeout = TimeSpan.FromSeconds(20),
					OutputRegistrationTimeout = TimeSpan.FromSeconds(20),
					TransactionSigningTimeout = TimeSpan.FromSeconds(20),
					MaxSuggestedAmountBase = Money.Satoshis(ProtocolConstants.MaxAmountPerAlice)
				});
				services.AddSingleton(_ => new CoinJoinScriptStore(usedScripts));
			})).CreateClient();
		var api = _apiApplicationFactory.CreateWabiSabiHttpApiClient(http);
		using var updater = RoundStateUpdaterForTesting.Create(api, timeout.Token);
		var provider = new RoundStateProvider(updater);
		var round = await provider.CreateRoundAwaiterAsync(r => r.Phase == Phase.InputRegistration, timeout.Token);
		var tasks = keys.Select((key, i) => WabiSabiFactory.CreateTestCoinJoinClient(_ => api, key, provider)
			.StartRoundAsync(coins[i], UnrestrictedRound.Instance, round, timeout.Token)).ToArray();
		var results = await Task.WhenAll(tasks);
		Assert.All(results, result => Assert.IsNotType<SuccessfulCoinJoinResult>(result));
		Assert.False(broadcast);
	}

	[Theory]
	[InlineData(new long[] { 30_000_000, 31_000_000, 32_000_000, 33_000_000, 34_000_000, 35_000_000, 36_000_000 }, new long[] { 50_000_000, 51_000_000, 52_000_000, 53_000_000, 54_000_000, 55_000_000, 56_000_000 }, new long[] { 70_000_000, 71_000_000, 72_000_000, 73_000_000, 74_000_000, 75_000_000, 76_000_000 })]
	public async Task CoinJoinWithBlameRoundTestAsync(long[] satAmounts1, long[] satAmounts2, long[] satAmounts3)
	{
		int inputCount = satAmounts1.Length;

		// At the end of the test a coinjoin transaction has to be created and broadcasted.
		var broadcastedTxTcs = new TaskCompletionSource<Transaction>(TaskCreationOptions.RunContinuationsAsynchronously);

		// Total test timeout.
		using var cts = new CancellationTokenSource(TimeSpan.FromMinutes(5));
		cts.Token.Register(() => broadcastedTxTcs.TrySetCanceled(), useSynchronizationContext: false);

		KeyManager keyManager1 = KeyManager.CreateNew(out var _, password: "", Network.Main);
		KeyManager keyManager2 = KeyManager.CreateNew(out var _, password: "", Network.Main);
		KeyManager keyManager3 = KeyManager.CreateNew(out var _, password: "", Network.Main);
		KeyManager keyManager4 = KeyManager.CreateNew(out _, "", Network.Main);

		// Four participants supply 28 inputs. One withholds signatures, leaving 21 in blame.
		var participant1Coins = GenerateSmartCoins(keyManager1, satAmounts1, inputCount);
		var participant2CoinsBad = GenerateSmartCoins(keyManager2, satAmounts2, inputCount);
		var participant3Coins = GenerateSmartCoins(keyManager3, satAmounts3, inputCount);
		var participant4Coins = GenerateSmartCoins(keyManager4, satAmounts1, inputCount);

		var coordinatorApp = _apiApplicationFactory.WithWebHostBuilder(builder =>
			builder.AddMockRpcClient(
				Enumerable.Concat(participant1Coins, participant2CoinsBad).Concat(participant3Coins).Concat(participant4Coins).ToImmutableList(),
				rpc =>
				{
					rpc.OnGetRawTransactionAsync = (txid, throwIfNotFound) =>
					{
						var tx = Transaction.Create(Network.Main);
						return Task.FromResult(tx);
					};

					// Make the coordinator believe that the transaction is being
					// broadcasted using the RPC interface. Once we receive this tx
					// (the `SendRawTransactionAsync` was invoked) we stop waiting
					// and finish the waiting tasks to finish the test successfully.
					rpc.OnSendRawTransactionAsync = (tx) =>
					{
						broadcastedTxTcs.SetResult(tx);
						return tx.GetHash();
					};
				})
			.ConfigureServices(services =>

				// Instruct the coordinator DI container to use this scoped
				// services to build everything (WabiSabi controller, arena, etc)
				services.AddSingleton(s => new WabiSabiConfig
				{
					AllowP2trInputs = true,
					AllowP2trOutputs = true,
					MaxInputCountByRound = 4 * inputCount,
					MinInputCountByRoundMultiplier = 0.75,
					MinInputCountByBlameRoundMultiplier = 0.75,
					// The coordinator opens another round in the final minute. Leave
					// enough time for all four clients to choose this normal round.
					StandardInputRegistrationTimeout = TimeSpan.FromMinutes(2),
					BlameInputRegistrationTimeout = TimeSpan.FromSeconds(30),
					ConnectionConfirmationTimeout = TimeSpan.FromSeconds(30),
					OutputRegistrationTimeout = TimeSpan.FromSeconds(30),
					TransactionSigningTimeout = TimeSpan.FromSeconds(4 * inputCount),
					MaxSuggestedAmountBase = Money.Satoshis(ProtocolConstants.MaxAmountPerAlice)
				})));

		await Task.Delay(100);

		// Create the coinjoin client
		var apiClient1 = _apiApplicationFactory.CreateWabiSabiHttpApiClient(coordinatorApp.CreateClient());
		using var roundStateUpdater = RoundStateUpdaterForTesting.Create(apiClient1, cts.Token);
		var roundStateProvider = new RoundStateProvider(roundStateUpdater);

		var roundState = await roundStateProvider.CreateRoundAwaiterAsync(roundState => roundState.Phase == Phase.InputRegistration, cts.Token);

		var httpClient = coordinatorApp.CreateClient();

		// Creates a mocked HttpClient that says everything is okay when a signature is sent but it doesn't really send it.
		using var nonSigningHttpClientMock = new MockHttpClient();
		nonSigningHttpClientMock.BaseAddress = httpClient.BaseAddress;
		nonSigningHttpClientMock.OnSendAsync = req =>
		{
			Assert.NotNull(req.RequestUri);

			if (req.RequestUri.ToString().Contains("transaction-signature"))
			{
				return Task.FromResult(new HttpResponseMessage(HttpStatusCode.OK));
			}

			return httpClient.SendAsync(req, CancellationToken.None);
		};

		var apiClient2Bad = _apiApplicationFactory.CreateWabiSabiHttpApiClient(nonSigningHttpClientMock);
		var apiClient3 = _apiApplicationFactory.CreateWabiSabiHttpApiClient(coordinatorApp.CreateClient());
		var apiClient4 = _apiApplicationFactory.CreateWabiSabiHttpApiClient(coordinatorApp.CreateClient());

		var signingTrace = new ConcurrentQueue<string>();
		var elapsed = System.Diagnostics.Stopwatch.StartNew();
		CoinJoinClient CreateParticipant(int participant, IWabiSabiApiRequestHandler api, KeyManager keys) =>
			WabiSabiFactory.CreateTestCoinJoinClient(_ => api,
				new TimedSyntheticKeyChain(new SyntheticKeyChain(keys), participant, signingTrace, elapsed),
				new OutputProvider(new InternalDestinationProvider(keys), RandomnessProviders.Insecure), roundStateProvider);
		var coinJoinClient1 = CreateParticipant(1, apiClient1, keyManager1);
		var coinJoinClient2Bad = CreateParticipant(2, apiClient2Bad, keyManager2);
		var coinJoinClient3 = CreateParticipant(3, apiClient3, keyManager3);
		var coinJoinClient4 = CreateParticipant(4, apiClient4, keyManager4);
		CoinJoinClient[] clients = [coinJoinClient1, coinJoinClient2Bad, coinJoinClient3, coinJoinClient4];
		var progress = clients.Select(_ => new ConcurrentQueue<string>()).ToArray();
		var endedRounds = clients.Select(_ => new ConcurrentQueue<RoundState>()).ToArray();
		static string DescribeRound(RoundState state) => $"id={state.Id}, blameOf={state.BlameOf}, phase={state.Phase}, end={state.EndRoundState}, inputs={state.CoinjoinState.Inputs.Count()}, outputs={state.CoinjoinState.Outputs.Count()}";
		for (int i = 0; i < clients.Length; i++)
		{
			var messages = progress[i];
			var rounds = endedRounds[i];
			clients[i].CoinJoinClientProgress += (_, value) =>
			{
				var state = value switch
				{
					global::MagicalCryptoWallet.WabiSabi.Client.CoinJoinProgressEvents.RoundEnded ended => ended.LastRoundState,
					global::MagicalCryptoWallet.WabiSabi.Client.CoinJoinProgressEvents.RoundStateChanged changed => changed.RoundState,
					_ => null
				};
				if (value is global::MagicalCryptoWallet.WabiSabi.Client.CoinJoinProgressEvents.RoundEnded && state is not null)
				{
					rounds.Enqueue(state);
				}
				messages.Enqueue($"{elapsed.Elapsed.TotalSeconds:F3}s {value.GetType().Name} {(state is null ? "" : DescribeRound(state))}");
			};
		}

		var participant1CoinjoinTask = coinJoinClient1.StartCoinJoinAsync(() => participant1Coins, cts.Token);
		var participant2CoinjoinTaskBad = coinJoinClient2Bad.StartRoundAsync(participant2CoinsBad, UnrestrictedRound.Instance, roundState, cts.Token);
		var participant3CoinjoinTask = coinJoinClient3.StartCoinJoinAsync(() => participant3Coins, cts.Token);
		var participant4CoinjoinTask = coinJoinClient4.StartCoinJoinAsync(() => participant4Coins, cts.Token);

		await Task.WhenAll(participant2CoinjoinTaskBad, participant1CoinjoinTask, participant3CoinjoinTask, participant4CoinjoinTask);

		var participant1Result = await participant1CoinjoinTask;
		var participant2ResultBad = await participant2CoinjoinTaskBad;
		var participant3Result = await participant3CoinjoinTask;
		var participant4Result = await participant4CoinjoinTask;
		if (participant1Result is not SuccessfulCoinJoinResult || participant3Result is not SuccessfulCoinJoinResult ||
			participant4Result is not SuccessfulCoinJoinResult)
		{
			foreach (var entry in signingTrace) { _output.WriteLine(entry); }
			CoinJoinResult[] results = [participant1Result, participant2ResultBad, participant3Result, participant4Result];
			for (int i = 0; i < clients.Length; i++)
			{
				_output.WriteLine($"Blame participant {i + 1}: result={results[i].GetType().Name}, progress={string.Join(", ", progress[i])}, schedules={string.Join(", ", ((TestableCoinJoinClient)clients[i]).ScheduledMaximumDelays)}.");
				foreach (var ended in endedRounds[i])
				{
					_output.WriteLine($"Blame participant {i + 1} retained RoundEnded: {DescribeRound(ended)}.");
				}
			}
			try
			{
				using var diagnosticTimeout = new CancellationTokenSource(TimeSpan.FromSeconds(5));
				var status = await apiClient1.GetStatusAsync(RoundStateRequest.Empty, diagnosticTimeout.Token);
				foreach (var current in status.RoundStates)
				{
					_output.WriteLine($"Blame coordinator round: id={current.Id}, blameOf={current.BlameOf}, phase={current.Phase}, end={current.EndRoundState}, inputs={current.CoinjoinState.Inputs.Count()}, outputs={current.CoinjoinState.Outputs.Count()}.");
				}
				var logPath = Path.GetFullPath(global::MagicalCryptoWallet.Logging.Logger.FilePath);
				if (logPath.StartsWith(Path.GetFullPath(Common.DataDir) + Path.DirectorySeparatorChar, StringComparison.Ordinal) && File.Exists(logPath))
				{
					// The immutable test build artifact retains complete exception stacks and
					// request timestamps, which the short console excerpt cannot preserve.
					File.Copy(logPath, Path.Combine(AppContext.BaseDirectory, "coinjoin-blame-synthetic.log"), overwrite: true);
					_output.WriteLine("Synthetic blame coordinator/client log tail:");
					foreach (var line in File.ReadLines(logPath).Where(line => line.Contains("CoinJoinClient") || line.Contains("Arena") || line.Contains("RoundStateUpdater")).TakeLast(400))
					{
						_output.WriteLine(line);
					}
				}
			}
			catch (Exception diagnosticError)
			{
				_output.WriteLine($"Blame diagnosis unavailable: {diagnosticError.GetType().Name}.");
			}
		}

		Assert.IsType<SuccessfulCoinJoinResult>(participant1Result);

		// The mock acknowledges signatures without forwarding them to the coordinator.
		// Its local result can be disrupted or failed, but it must never succeed.
		Assert.IsNotType<SuccessfulCoinJoinResult>(participant2ResultBad);

		Assert.IsType<SuccessfulCoinJoinResult>(participant3Result);
		Assert.IsType<SuccessfulCoinJoinResult>(participant4Result);

		var broadcastedTx = await broadcastedTxTcs.Task; // wait for the transaction to be broadcasted.
		Assert.NotNull(broadcastedTx);
		Assert.True(broadcastedTx.Inputs.Count >= 21);

		// Only coins of the first and the third participant are expected here. The second one failed to register outputs and was blamed.
		var expectedInputs = participant1Coins.Concat(participant3Coins).Concat(participant4Coins)
			.Select(x => x.Coin.Outpoint.ToString())
			.Order()
			.ToList();

		var actualInputs = broadcastedTx.Inputs
			.Select(x => x.PrevOut.ToString())
			.Order();

		Assert.Equal(expectedInputs, actualInputs);
	}

	[Theory]
	[InlineData(123456, 0.00, 0.00)]
	public async Task MultiClientsCoinJoinTestAsync(
		int seed,
		double faultInjectorMonkeyAggressiveness,
		double delayInjectorMonkeyAggressiveness)
	{
		// Total test timeout.
		using var cts = new CancellationTokenSource(TimeSpan.FromMinutes(2));

		const int NumberOfParticipants = 21;
		const int NumberOfCoinsPerParticipant = 1;
		const int ExpectedInputNumber = NumberOfParticipants * NumberOfCoinsPerParticipant;

		var coinJoinBroadcasted = new TaskCompletionSource<Transaction>(TaskCreationOptions.RunContinuationsAsynchronously);
		var rpc = BitcoinFactory.GetMockMinimalRpc();
		var onSendRawTransaction = rpc.OnSendRawTransactionAsync;
		rpc.OnSendRawTransactionAsync = tx =>
		{
			onSendRawTransaction?.Invoke(tx);
			if (tx.Inputs.Count > 1)
			{
				coinJoinBroadcasted.SetResult(tx);
			}

			return tx.GetHash();
		};
		var coordinatorApp = _apiApplicationFactory.WithWebHostBuilder(builder =>
			builder.ConfigureServices(services =>
			{
				// Instruct the coordinator DI container to use these two scoped
				// services to build everything (WabiSabi controller, arena, etc)
				services.AddSingleton<IRPCClient>(s => rpc);
				services.AddSingleton(s => new WabiSabiConfig(Path.GetTempFileName())
				{
					MaxRegistrableAmount = Money.Coins(500m),
					MaxInputCountByRound = ExpectedInputNumber,
					MinInputCountByRoundMultiplier = 1,
					StandardInputRegistrationTimeout = TimeSpan.FromSeconds(5 * ExpectedInputNumber),
					BlameInputRegistrationTimeout = TimeSpan.FromSeconds(2 * ExpectedInputNumber),
					ConnectionConfirmationTimeout = TimeSpan.FromSeconds(2 * ExpectedInputNumber),
					OutputRegistrationTimeout = TimeSpan.FromSeconds(5 * ExpectedInputNumber),
					TransactionSigningTimeout = TimeSpan.FromSeconds(3 * ExpectedInputNumber),
					MaxSuggestedAmountBase = Money.Satoshis(ProtocolConstants.MaxAmountPerAlice)
				});
			}));

		var httpClient = coordinatorApp.CreateClient();

		await Task.Delay(100);
		using var httpClientWrapper = new MonkeyHttpClient(
			httpClient,
			() => // This monkey injects `HttpRequestException` randomly to simulate errors in the communication.
			{
				if (Random.Shared.NextDouble() < faultInjectorMonkeyAggressiveness)
				{
					throw new HttpRequestException("Crazy monkey hates you, donkey.");
				}
				return Task.CompletedTask;
			},
			async () => // This monkey injects `Delays` randomly to simulate slow response times.
			{
				await Task.Delay(TimeSpan.FromSeconds(5 * delayInjectorMonkeyAggressiveness), cts.Token).ConfigureAwait(false);
			});
		httpClientWrapper.BaseAddress = httpClient.BaseAddress;

		var apiClient = new WabiSabiHttpApiClient("", new MockHttpClientFactory {OnCreateClient = _ => httpClientWrapper});

		var participants = Enumerable
			.Range(0, NumberOfParticipants)
			.Select(i => new Participant($"participant{i}", rpc, _ => apiClient))
			.ToArray();

		foreach (var participant in participants)
		{
			await participant.GenerateSourceCoinAsync(cts.Token);
		}
		var dummyWallet = new TestWallet("dummy", rpc);
		await dummyWallet.GenerateAsync(101, cts.Token);
		foreach (var participant in participants)
		{
			await participant.GenerateCoinsAsync(NumberOfCoinsPerParticipant, seed, cts.Token);
		}
		await dummyWallet.GenerateAsync(101, cts.Token);

		var tasks = participants.Select(x => x.StartParticipatingAsync(cts.Token)).ToArray();

		var coinjoinTransactionCompletionTask = coinJoinBroadcasted.Task.WaitAsync(cts.Token);
		var participantsFinishedTask = Task.WhenAll(tasks);
		var finishedTask = await Task.WhenAny(participantsFinishedTask, coinjoinTransactionCompletionTask);
		_output.WriteLine($"CoinJoin outcomes: broadcast={coinjoinTransactionCompletionTask.Status}, participants completed={tasks.Count(t => t.IsCompletedSuccessfully)}, faulted={tasks.Count(t => t.IsFaulted)}, canceled={tasks.Count(t => t.IsCanceled)}.");
		for (int i = 0; i < participants.Length; i++)
		{
			_output.WriteLine($"Participant {i}: {string.Join(", ", participants[i].Progress)}; schedules={string.Join(", ", participants[i].ActiveClient?.ScheduledMaximumDelays.ToArray() ?? [])}.");
		}
		if (!coinJoinBroadcasted.Task.IsCompletedSuccessfully)
		{
			using var diagnosticTimeout = new CancellationTokenSource(TimeSpan.FromSeconds(5));
			var directApi = _apiApplicationFactory.CreateWabiSabiHttpApiClient(httpClient);
			var status = await directApi.GetStatusAsync(RoundStateRequest.Empty, diagnosticTimeout.Token);
			foreach (var round in status.RoundStates)
			{
				var parameters = round.CoinjoinState.Parameters;
				_output.WriteLine($"Coordinator round: phase={round.Phase}, end={round.EndRoundState}, inputs={round.CoinjoinState.Inputs.Count()}, outputs={round.CoinjoinState.Outputs.Count()}, inputTimeout={round.InputRegistrationTimeout}, confirmationTimeout={parameters.ConnectionConfirmationTimeout}, outputTimeout={parameters.OutputRegistrationTimeout}, signingTimeout={parameters.TransactionSigningTimeout}, delaySigning={parameters.DelayTransactionSigning}.");
			}
		}
		foreach (var faulted in tasks.Where(t => t.IsFaulted))
		{
			_output.WriteLine($"Participant fault: {faulted.Exception}");
		}

		if (finishedTask == coinjoinTransactionCompletionTask)
		{
			var broadcastedCoinjoinTransaction = await coinjoinTransactionCompletionTask;
			var mempool = await rpc.GetRawMempoolAsync();
			var coinjoinFromMempool = await rpc.GetRawTransactionAsync(mempool.Single(), cancellationToken: cts.Token);

			Assert.Equal(broadcastedCoinjoinTransaction.GetHash(), coinjoinFromMempool.GetHash());
			Assert.True(broadcastedCoinjoinTransaction.Inputs.Count >= 21);
		}
		else if (finishedTask == participantsFinishedTask)
		{
			var participantsFinishedSuccessfully = tasks
				.Where(t => t.IsCompletedSuccessfully)
				.Select(t => t.Result)
				.ToArray();

			// In case some participants claim to have finished successfully then wait a second for seeing
			// the coinjoin in the mempool. This seems really hard to believe but just in case.
			if (participantsFinishedSuccessfully.Length > 0 && participantsFinishedSuccessfully.All(x => x is SuccessfulCoinJoinResult))
			{
				await Task.Delay(TimeSpan.FromSeconds(1), cts.Token);
				var mempool = await rpc.GetRawMempoolAsync(cts.Token);
				Assert.Single(mempool);
			}
			else if (participantsFinishedSuccessfully.Length > 0 && participantsFinishedSuccessfully.All(x => x is FailedCoinJoinResult))
			{
				throw new Exception("All participants finished, but CoinJoin still not in the mempool (no more blame rounds).");
			}
			else if (participantsFinishedSuccessfully.Length == 0)
			{
				var exceptions = tasks
					.Where(x => x.IsFaulted)
					.Select(x => new Exception("Something went wrong", x.Exception))
					.ToArray();
				if (exceptions.Length == 0) { throw new OperationCanceledException(cts.Token); }
				throw new AggregateException(exceptions);
			}
			else
			{
				throw new Exception("All participants finished, but CoinJoin still not in the mempool.");
			}
		}
		else
		{
			throw new Exception("This is not so possible.");
		}

		// Real registration, output, readiness and signing requests must all use
		// the fixture's schedule. Bypassing it restores long random production
		// sleeps inside this test's unchanged two-minute deadline.
		Assert.All(participants, participant =>
		{
			var delays = participant.ActiveClient!.ScheduledMaximumDelays.ToArray();
			Assert.Contains(TimeSpan.MaxValue, delays);
			Assert.Contains(TimeSpan.FromSeconds(50), delays);
			Assert.True(delays.Count(delay => delay == TimeSpan.FromSeconds(10)) >= 2);
		});
	}

	[Fact]
	public async Task RegisterCoinIdempotencyAsync()
	{
		using var signingKey = new Key();
		Coin coinToRegister = new(
			fromOutpoint: BitcoinFactory.CreateOutPoint(),
			fromTxOut: new TxOut(Money.Coins(1), signingKey.PubKey.GetScriptPubKey(ScriptPubKeyType.Segwit)));

		using var httpClient = _apiApplicationFactory.WithWebHostBuilder(builder =>
			builder.ConfigureServices(services =>
			{
				var rpc = BitcoinFactory.GetMockMinimalRpc();
				rpc.OnGetTxOutAsync = (_, _, _) => new()
				{
					Confirmations = 101,
					IsCoinBase = false,
					ScriptPubKeyType = "witness_v0_keyhash",
					TxOut = coinToRegister.TxOut
				};
				rpc.OnGetRawTransactionAsync = (txid, throwIfNotFound) =>
				{
					var tx = Transaction.Create(Network.Main);
					return Task.FromResult(tx);
				};
				services.AddSingleton<IRPCClient>(s => rpc);
			})).CreateClient();

		var apiClient = await _apiApplicationFactory.CreateArenaClientAsync(httpClient);
		var rounds = (await apiClient.GetStatusAsync(RoundStateRequest.Empty, CancellationToken.None)).RoundStates;
		var round = rounds.First(x => x.CoinjoinState is ConstructionState);
		using var stutteredHttpClient = new StuttererHttpClient(httpClient);
		var stutteredApiClient = new ArenaClient(
			apiClient.AmountCredentialClient,
			apiClient.VsizeCredentialClient,
			apiClient.CoordinatorIdentifier,
			_apiApplicationFactory.CreateWabiSabiHttpApiClient(stutteredHttpClient));

		var ownershipProof = WabiSabiFactory.CreateOwnershipProof(signingKey, round.Id);
		var response = await stutteredApiClient.RegisterInputAsync(round.Id, coinToRegister.Outpoint, ownershipProof, CancellationToken.None);

		Assert.NotEqual(Guid.Empty, response.Value);
	}

	private sealed class TimedSyntheticKeyChain(IKeyChain inner, int participant, ConcurrentQueue<string> trace,
		System.Diagnostics.Stopwatch elapsed) : IKeyChain
	{
		public OwnershipProof GetOwnershipProof(IDestination destination, CoinJoinInputCommitmentData committedData) =>
			inner.GetOwnershipProof(destination, committedData);

		public Transaction Sign(Transaction transaction, Coin coin, PrecomputedTransactionData precomputeTransactionData)
		{
			var started = elapsed.Elapsed;
			try { return inner.Sign(transaction, coin, precomputeTransactionData); }
			finally { trace.Enqueue($"Blame signing participant {participant}: started={started.TotalSeconds:F3}s, finished={elapsed.Elapsed.TotalSeconds:F3}s, duration={(elapsed.Elapsed - started).TotalSeconds:F3}s."); }
		}
	}

	private ImmutableList<SmartCoin> GenerateSmartCoins(KeyManager keyManager, long[] amounts, int inputCount)
	{
				return keyManager.GetKeys()
			.Take(inputCount)
			.Select((x, i) =>
			{
				return BitcoinFactory.CreateSmartCoin(x, Money.Satoshis(amounts[i]), true, 1);
			})
			.ToImmutableList();
	}

	public class TestableRpcClient : RpcClientBase
	{
		public TestableRpcClient(RpcClientBase rpc)
			: base(rpc.RpcClient)
		{
		}

		public Action<Transaction>? AfterSendRawTransaction { get; set; }

		public override async Task<uint256> SendRawTransactionAsync(Transaction transaction, CancellationToken cancellationToken = default)
		{
			var ret = await base.SendRawTransactionAsync(transaction, cancellationToken).ConfigureAwait(false);
			AfterSendRawTransaction?.Invoke(transaction);
			return ret;
		}
	}
}
