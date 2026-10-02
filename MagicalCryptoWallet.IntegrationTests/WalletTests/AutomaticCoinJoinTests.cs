using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Security;
using System.Threading;
using System.Threading.Channels;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Coordinator.WabiSabi;
using MagicalCryptoWallet.FeeRateEstimation;
using MagicalCryptoWallet.IntegrationTests.Infrastructure;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.WabiSabi.Client.Banning;
using MagicalCryptoWallet.WabiSabi.Client.Batching;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Client.RoundStateAwaiters;
using MagicalCryptoWallet.WabiSabi.Coordinator;
using MagicalCryptoWallet.WabiSabi.Coordinator.DoSPrevention;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.IntegrationTests.WalletTests;

[Collection("Integration tests")]
public class AutomaticCoinJoinTests(IntegrationTestFixture fixture, ITestOutputHelper output)
{
	[Fact(Timeout = 900_000)]
	public async Task SevenEncryptedWalletsAuthorizeThroughSendAndConfirmFirstRoundPaymentsAsync()
	{
		using var timeout = CancellationTokenSource.CreateLinkedTokenSource(TestContext.Current.CancellationToken);
		timeout.CancelAfter(TimeSpan.FromMinutes(12));
		var cancel = timeout.Token;
		var config = new WabiSabiConfig
		{
			Network = Network.RegTest,
			MaxInputCountByRound = 70,
			MinInputCountByRoundMultiplier = 0.3,
			StandardInputRegistrationTimeout = TimeSpan.FromMinutes(3),
			ConnectionConfirmationTimeout = TimeSpan.FromMinutes(1),
			OutputRegistrationTimeout = TimeSpan.FromMinutes(1),
			TransactionSigningTimeout = TimeSpan.FromMinutes(1),
			MaxSuggestedAmountBase = Money.Coins(100),
			CollectCoordinatorFees = false
		};
		Assert.Equal(21, config.MinInputCountByRound);
		var offenders = Channel.CreateUnbounded<Offender>();
		var prison = new Prison([], offenders.Writer);
		// Anchor the estimate at the shortest supported range; 108 is rounded up to
		// 144 by FeeRateEstimations and cannot satisfy the coordinator's 108 target.
		var estimations = new FeeRateEstimations(new Dictionary<int, FeeRate> { [2] = new(2m) });
		Assert.Equal(2m, estimations.GetFeeRate((int)config.ConfirmationTarget).SatoshiPerByte);
		FeeRateProvider fees = _ => Task.FromResult(estimations);
		using var arena = new Arena(config, fixture.BitcoinCoreNode.RpcClient, prison,
			(rate, maximum, minimum) => RoundParameters.Create(config, rate, maximum, minimum), fees,
			period: TimeSpan.FromMilliseconds(100));
		var clients = new List<Client>();
		var sendTransactions = new HashSet<uint256>();
		var payments = new List<(Client Client, Guid Id, BitcoinAddress Address)>();
		try
		{
			for (var index = 0; index < 7; index++)
			{
				var environment = await RegTestEnvironment.CreateAsync(fixture, callerMemberName: "seven-clients-" + Guid.NewGuid().ToString("N"));
				var client = await Client.CreateAsync(environment, arena, index, config.CoordinatorIdentifier, cancel);
				clients.Add(client);
				for (var funding = 0; funding < 4; funding++)
				{
					var key = client.Wallet.KeyManager.GetNextReceiveKey("synthetic funding " + funding);
					await environment.FundAddressAsync(key.GetP2wpkhAddress(Network.RegTest), Money.Coins(1), confirmations: 0);
				}
			}
			await fixture.WalletRpcClient.GenerateAsync(1, cancel);
			await Task.WhenAll(clients.Select(client => client.SyncAsync(cancel)));
			foreach (var client in clients)
			{
				Assert.True(client.Session.Snapshot.CoinJoinRequiresAuthorization);
				Assert.True(client.Session.Snapshot.IsSynchronized);
				Assert.Equal(CoinJoinClientState.Idle, client.Manager.Snapshot.State);
				var destination = await fixture.WalletRpcClient.GetNewAddressAsync(cancellationToken: cancel);
				var preview = client.Wallet.BuildTransaction(new Destination(destination.ScriptPubKey), Money.Satoshis(500_000),
					"synthetic send", new FeeRate(2m), client.Wallet.Coins, subtractFee: false);
				using (var authorization = WalletAuthorization.Create(client.Wallet.KeyManager, Client.Password))
				{
					var signed = authorization.Sign(preview);
					client.Session.CompleteOperationAuthorization(authorization);
					var txid = await fixture.BitcoinCoreNode.RpcClient.SendRawTransactionAsync(signed.Transaction.Transaction, cancel);
					sendTransactions.Add(txid);
				}
				Assert.False(client.Session.Snapshot.CoinJoinRequiresAuthorization);
				Assert.Throws<SecurityException>(() => WalletAuthorization.Create(client.Wallet.KeyManager, "incorrect password"));
				var paymentAddress = await fixture.WalletRpcClient.GetNewAddressAsync(cancellationToken: cancel);
				payments.Add((client, Guid.Parse(client.Wallet.AddCoinJoinPayment(paymentAddress, Money.Satoshis(500_000))), paymentAddress));
			}
			await fixture.WalletRpcClient.GenerateAsync(1, cancel);
			await Task.WhenAll(clients.Select(client => client.SyncAsync(cancel)));

			// Open the coordinator only when all seven automatic clients are waiting
			// for a round, so no participant misses its first broadcast.
			foreach (var client in clients)
			{
				await client.Environment.WaitForConditionAsync(() => client.Manager.Snapshot.State == CoinJoinClientState.InProgress, TimeSpan.FromMinutes(4));
			}
			await arena.StartAsync(cancel);
			await clients[0].Environment.WaitForConditionAsync(() => arena.Rounds.Count > 0, TimeSpan.FromSeconds(15));
			Transaction? firstCoinJoin = null;
			uint256? firstCoinJoinBlock = null;
			while (firstCoinJoin is null)
			{
				cancel.ThrowIfCancellationRequested();
				foreach (var hash in await fixture.BitcoinCoreNode.RpcClient.GetRawMempoolAsync(cancel))
				{
					if (sendTransactions.Contains(hash)) { continue; }
					var transaction = await fixture.BitcoinCoreNode.RpcClient.GetRawTransactionAsync(hash, cancellationToken: cancel);
					if (transaction.Inputs.Count >= 21)
					{
						firstCoinJoin = transaction;
						firstCoinJoinBlock = (await fixture.WalletRpcClient.GenerateAsync(1, cancel))[0];
						break;
					}
				}
				await Task.Delay(250, cancel);
			}
			Assert.NotNull(firstCoinJoinBlock);
			Assert.True(firstCoinJoin.Inputs.Count >= 21);
			foreach (var client in clients) { client.Manager.RequestCoinJoinStop(); }
			await Task.WhenAll(clients.Select(client => client.SyncAsync(cancel)));
			foreach (var (client, id, address) in payments)
			{
				await client.Environment.WaitForConditionAsync(
					() => client.Wallet.BatchedPayments.GetPayments().Single(p => p.Id == id).State is FinishedPayment,
					TimeSpan.FromMinutes(1));
				var payment = Assert.Single(client.Wallet.BatchedPayments.GetPayments(), p => p.Id == id);
				var finished = Assert.IsType<FinishedPayment>(payment.State);
				Assert.Equal(firstCoinJoin.GetHash(), finished.TransactionId);
				var paidOutput = Assert.Single(firstCoinJoin.Outputs, o => o.ScriptPubKey == address.ScriptPubKey);
				Assert.Equal(Money.Satoshis(500_000), paidOutput.Value);
				var finishedStates = 0;
				for (PaymentState? state = payment.State; state is not null; state = state.PreviousState)
				{
					if (state is FinishedPayment) { finishedStates++; }
				}
				Assert.Equal(1, finishedStates);
				await client.Environment.WaitForConditionAsync(
					() => client.Wallet.Coins.Any(c => c.Confirmed && c.HdPubKey.AnonymitySet >= 2),
					TimeSpan.FromMinutes(1));
			}
			output.WriteLine($"Seven encrypted clients confirmed CoinJoin {firstCoinJoin.GetHash()} with {firstCoinJoin.Inputs.Count} inputs; all seven payments appeared exactly once in its first round.");
		}
		finally
		{
			try { await Task.WhenAll(clients.Select(client => client.DisposeAsync().AsTask())); }
			finally { await arena.StopAsync(CancellationToken.None); }
		}
	}

	private sealed class Client : IAsyncDisposable
	{
		public const string Password = "synthetic integration password";
		public required RegTestEnvironment Environment { get; init; }
		public required WalletSession Session { get; init; }
		public required CoinJoinManager Manager { get; init; }
		private MailboxProcessor<RoundUpdateMessage> Rounds { get; init; } = null!;
		private CoinPrison Prison { get; init; } = null!;
		private Timer Timer { get; init; } = null!;
		public Wallet Wallet => Session.GetWallet()!;

		public static async Task<Client> CreateAsync(RegTestEnvironment environment, Arena arena, int index, string coordinatorIdentifier, CancellationToken cancel)
		{
			WalletSession? session = null;
			MailboxProcessor<RoundUpdateMessage>? rounds = null;
			Timer? timer = null;
			CoinPrison? prison = null;
			CoinJoinManager? manager = null;
			try
			{
				var directory = new WalletDirectories(Network.RegTest, Path.Combine(environment.WorkDir, "encrypted client " + index));
				var setup = new WalletSession(Network.RegTest, directory, keys => environment.CreateWallet(keys));
				var draft = new WalletGenerator(directory.WalletsDir, Network.RegTest).GenerateDraft(Password, (Mnemonic?)null).KeyManager;
				setup.Configure(draft, Password);
				await setup.StopAsync(cancel);
				session = new WalletSession(Network.RegTest, directory, keys => environment.CreateWallet(keys));
				Assert.True(session.Snapshot.CoinJoinRequiresAuthorization);
				await session.InitializeAsync(cancel);
				rounds = new MailboxProcessor<RoundUpdateMessage>("SyntheticRoundObserver" + index,
					Workers.EventDriven(new RoundsState(DateTime.MinValue, TimeSpan.FromMilliseconds(250), new(), []), RoundStateUpdater.Create(arena)), cancellationToken: cancel);
				rounds.Start();
				timer = new Timer(_ => rounds.Post(new RoundUpdateMessage.UpdateMessage(DateTime.UtcNow)), null, TimeSpan.Zero, TimeSpan.FromMilliseconds(100));
				prison = CoinPrison.CreateOrLoadFromFile(environment.WorkDir);
				manager = new CoinJoinManager(session, new RoundStateProvider(rounds), _ => arena,
					new CoinJoinConfiguration(coordinatorIdentifier, 50m), prison, environment.EventBus);
				await manager.StartAsync(cancel);
				return new() { Environment = environment, Session = session, Manager = manager, Rounds = rounds, Timer = timer, Prison = prison };
			}
			catch
			{
				timer?.Dispose();
				using var cleanup = new CancellationTokenSource(TimeSpan.FromSeconds(30));
				try { if (manager is not null) { await manager.StopAsync(cleanup.Token); } }
				finally
				{
					manager?.Dispose();
					rounds?.Dispose();
					prison?.Dispose();
					try { if (session is not null) { await session.StopAsync(cleanup.Token); } }
					finally { await environment.DisposeAsync(); }
				}
				throw;
			}
		}

		public async Task SyncAsync(CancellationToken cancel)
		{
			await Environment.SyncFiltersP2PAsync(cancel);
			await Environment.WaitForConditionAsync(() => Session.Snapshot.IsSynchronized, TimeSpan.FromMinutes(1));
			var expected = await Environment.RpcClient.GetBlockCountAsync(cancel);
			await Environment.WaitForConditionAsync(() => Session.Snapshot.SyncHeight == expected, TimeSpan.FromMinutes(1));
		}

		public async ValueTask DisposeAsync()
		{
			Timer.Dispose();
			using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
			try { await Manager.StopAsync(timeout.Token); }
			finally
			{
				Manager.Dispose();
				Rounds.Dispose();
				Prison.Dispose();
				try { await Session.StopAsync(timeout.Token); }
				finally { await Environment.DisposeAsync(); }
			}
		}
	}
}
