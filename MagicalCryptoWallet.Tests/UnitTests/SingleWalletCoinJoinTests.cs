using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Tests.UnitTests.Mocks;
using MagicalCryptoWallet.Tests.UnitTests.Services;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Client.Banning;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;
using MagicalCryptoWallet.WabiSabi.Client.RoundStateAwaiters;
using MagicalCryptoWallet.WabiSabi.Client.StatusChangedEvents;
using Xunit;
using System.Security;
using NBitcoin;
using MagicalCryptoWallet.Client.Rpc;
using MagicalCryptoWallet.Rpc;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class SingleWalletCoinJoinTests
{
	[Fact]
	public async Task SendingPasswordAuthorizesAutomaticCoinJoinAfterAllRestrictionsEndAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: true, password: "secret", automatic: true);
		var session = fixture.Application.Session;
		Assert.True(session.Snapshot.CoinJoinRequiresAuthorization);
		fixture.Manager.WalletEnteredSendWorkflow();
		await fixture.Manager.SignalToStopCoinjoinsAsync();
		using var recipient = new Key();
		var payments = new[] { new PaymentInfo { Sendto = new Destination(recipient.PubKey.GetAddress(ScriptPubKeyType.Segwit, Network.RegTest).ScriptPubKey), Amount = Money.Coins(0.01m), Label = "synthetic" } };
		var rpc = new MagicalCryptoWalletJsonRpcService(fixture.Application.Global);
		Assert.Throws<SecurityException>(() => rpc.BuildTransaction(payments, feeRate: 2m, password: "wrong"));
		Assert.True(session.Snapshot.CoinJoinRequiresAuthorization);
		Assert.NotEmpty(rpc.BuildTransaction(payments, feeRate: 2m, password: "secret"));
		Assert.False(session.Snapshot.CoinJoinRequiresAuthorization);
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.True(fixture.Manager.Snapshot.SendRestricted);
		Assert.Equal(CoinJoinClientState.Idle, fixture.Manager.Snapshot.State);
		Assert.Throws<SecurityException>(() => rpc.BuildTransaction(payments, feeRate: 2m, password: "wrong"));
		fixture.Manager.WalletLeftSendWorkflow();
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.InProgress);
		// The signing scope has already been disposed; CoinJoin owns its independent retained scope.
		Assert.NotNull(session.CoinJoinKeyChain);
		fixture.Manager.RequestCoinJoinStop();
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.Idle);
		using var anotherOperation = WalletAuthorization.Create(session.GetWallet()!.KeyManager, "secret");
		session.CompleteOperationAuthorization(anotherOperation);
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.Equal(CoinJoinClientState.Idle, fixture.Manager.Snapshot.State);
	}

	[Fact]
	public async Task SuccessfulOperationNeverEnablesDisabledAutomaticCoinJoinAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: true, password: "secret");
		using var authorization = WalletAuthorization.Create(fixture.Application.Session.GetWallet()!.KeyManager, "secret");
		fixture.Application.Session.CompleteOperationAuthorization(authorization);
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.False(fixture.Application.Session.Snapshot.CoinJoinRequiresAuthorization);
		Assert.Equal(CoinJoinClientState.Idle, fixture.Manager.Snapshot.State);
	}
	[Fact]
	public async Task ConcurrentStartsUseOneTrackerAndCanceledTrackerIsNeverRestartedAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: true);
		int starts = 0;
		fixture.Manager.StatusChanged += (_, status) => { if (status is StartedEventArgs) { Interlocked.Increment(ref starts); } };
		await Task.WhenAll(Enumerable.Range(0, 40).Select(_ => Task.Run(() => fixture.Manager.RequestCoinJoinStart(false, true))));
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.InProgress);
		await fixture.Manager.RestartAbortedCoinjoinsAsync(); // Barrier after all queued starts.
		Assert.Equal(1, starts);
		fixture.Manager.RequestCoinJoinStop();
		fixture.Manager.RequestCoinJoinStart(false, true);
		await SingleWalletTests.WaitForAsync(() => starts == 2 && fixture.Manager.Snapshot.State == CoinJoinClientState.InProgress);
		fixture.Manager.RequestCoinJoinStop();
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.Idle);
		Assert.Equal(2, starts);
	}

	[Fact]
	public async Task SendAndShutdownHoldsRemainIndependentAndManualStopCancelsResumeAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: false);
		fixture.Manager.RequestCoinJoinStart(false, true);
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.InSchedule);
		fixture.Manager.WalletEnteredSendWorkflow();
		fixture.Manager.WalletEnteredSendWorkflow();
		await fixture.Manager.WalletEnteredSendingAsync();
		await fixture.Manager.SignalToStopCoinjoinsAsync();
		fixture.Manager.WalletLeftSendWorkflow();
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.True(fixture.Manager.Snapshot.SendRestricted);
		Assert.False(fixture.Manager.Snapshot.ShutdownRestricted);
		Assert.Equal(CoinJoinClientState.Idle, fixture.Manager.Snapshot.State);
		fixture.Manager.WalletLeftSendWorkflow();
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.InSchedule);
		Assert.False(fixture.Manager.Snapshot.SendRestricted);
		fixture.Manager.RequestCoinJoinStop();
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.Equal(CoinJoinClientState.Idle, fixture.Manager.Snapshot.State);
		using var late = fixture.Manager.Subscribe(snapshot => Assert.Equal(CoinJoinClientState.Idle, snapshot.State));
	}

	private sealed class Fixture : IAsyncDisposable
	{
		private Fixture(SingleWalletTests.SyntheticApplication application, CoinJoinManager manager, MagicalCryptoWallet.Services.MailboxProcessor<RoundUpdateMessage> rounds, CoinPrison prison)
		{ Application = application; Manager = manager; _rounds = rounds; _prison = prison; }
		private readonly MagicalCryptoWallet.Services.MailboxProcessor<RoundUpdateMessage> _rounds;
		private readonly CoinPrison _prison;
		public SingleWalletTests.SyntheticApplication Application { get; }
		public CoinJoinManager Manager { get; }
		public static async Task<Fixture> CreateAsync(bool funded, string password = "", bool automatic = false)
		{
			var root = await Common.GetEmptyWorkDirAsync();
			var app = new SingleWalletTests.SyntheticApplication(root);
			var keys = app.NewKeys(password);
			keys.AutoCoinJoin = automatic;
			var wallet = app.Session.Configure(keys);
			if (funded)
			{
				foreach (var coin in ServiceFactory.CreateCoins(wallet.KeyManager, [("synthetic", 0, 0.1m, true, 1)]))
				{ wallet.TransactionProcessor.Process(coin.Transaction); wallet.TransactionStore.AddOrUpdate(coin.Transaction); }
			}
			await app.InitializeAsync();
			await SingleWalletTests.WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
			var api = new WabiSabiHttpApiClient("synthetic", new MockHttpClientFactory());
			var rounds = RoundStateUpdaterForTesting.CreateManual(api);
			var prison = CoinPrison.CreateOrLoadFromFile(root);
			var manager = new CoinJoinManager(app.Session, new RoundStateProvider(rounds), _ => api,
				new CoinJoinConfiguration("synthetic", 150m, 1, false), prison, InputVerifiers.NoVerification(), app.Events);
			await manager.StartAsync(CancellationToken.None);
			return new(app, manager, rounds, prison);
		}
		public async ValueTask DisposeAsync()
		{
			using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
			await Manager.StopAsync(timeout.Token);
			Manager.Dispose(); _rounds.Dispose(); _prison.Dispose(); await Application.DisposeAsync();
		}
	}
}
