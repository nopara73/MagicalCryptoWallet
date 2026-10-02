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
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Wallets;
using MagicalCryptoWallet.WabiSabi.Models;
using MagicalCryptoWallet.WabiSabi.Coordinator.PostRequests;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.WabiSabi.Coordinator;
using MagicalCryptoWallet.WabiSabi.Coordinator.Models;

namespace MagicalCryptoWallet.Tests.UnitTests;

[Collection("Serial unit tests collection")]
public class SingleWalletCoinJoinTests
{
	[Fact]
	public async Task SendingPasswordAuthorizesAutomaticCoinJoinAfterAllRestrictionsEndAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: true, password: "secret");
		var session = fixture.Application.Session;
		Assert.True(session.Snapshot.CoinJoinRequiresAuthorization);
		fixture.Manager.WalletEnteredSendWorkflow();
		await fixture.Manager.SignalToStopCoinjoinsAsync();
		using var recipient = new Key();
		var destination = recipient.PubKey.GetAddress(ScriptPubKeyType.Segwit, Network.RegTest).ScriptPubKey;
		Assert.Throws<SecurityException>(() => WalletOperationTestHelper.SignPayment(session, destination, Money.Coins(0.01m), "wrong"));
		Assert.True(session.Snapshot.CoinJoinRequiresAuthorization);
		Assert.True(WalletOperationTestHelper.SignPayment(session, destination, Money.Coins(0.01m), "secret").Signed);
		Assert.False(session.Snapshot.CoinJoinRequiresAuthorization);
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.True(fixture.Manager.Snapshot.SendRestricted);
		Assert.Equal(CoinJoinClientState.Idle, fixture.Manager.Snapshot.State);
		Assert.Throws<SecurityException>(() => WalletOperationTestHelper.SignPayment(session, destination, Money.Coins(0.01m), "wrong"));
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
	public async Task SuccessfulOperationImmediatelyStartsAutomaticCoinJoinAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: true, password: "secret");
		using var authorization = WalletAuthorization.Create(fixture.Application.Session.GetWallet()!.KeyManager, "secret");
		fixture.Application.Session.CompleteOperationAuthorization(authorization);
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.False(fixture.Application.Session.Snapshot.CoinJoinRequiresAuthorization);
		Assert.Equal(CoinJoinClientState.InProgress, fixture.Manager.Snapshot.State);
	}
	[Fact]
	public async Task ConcurrentStartsUseOneTrackerAndCanceledTrackerIsNeverRestartedAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: true);
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.InProgress);
		int starts = 0;
		fixture.Manager.StatusChanged += (_, status) => { if (status is StartedEventArgs) { Interlocked.Increment(ref starts); } };
		await Task.WhenAll(Enumerable.Range(0, 40).Select(_ => Task.Run(() => fixture.Manager.RequestCoinJoinStart(overridePlebStop: true))));
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.InProgress);
		await fixture.Manager.RestartAbortedCoinjoinsAsync(); // Barrier after all queued starts.
		Assert.Equal(0, starts);
		fixture.Manager.RequestCoinJoinStop();
		fixture.Manager.RequestCoinJoinStart(overridePlebStop: true);
		await SingleWalletTests.WaitForAsync(() => starts == 1 && fixture.Manager.Snapshot.State == CoinJoinClientState.InProgress);
		fixture.Manager.RequestCoinJoinStop();
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.Idle);
		Assert.Equal(1, starts);
	}

	[Fact]
	public async Task SendAndShutdownHoldsRemainIndependentAndManualStopCancelsResumeAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: false);
		fixture.Manager.RequestCoinJoinStart(overridePlebStop: true);
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

	[Theory]
	[InlineData(1, false)]
	[InlineData(2, true)]
	[InlineData(3, true)]
	public async Task FixedTargetAndQuietWaitingAsync(int score, bool isPrivate)
	{
		await using var fixture = await Fixture.CreateAsync(funded: true, score: score);
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		var wallet = fixture.Application.Session.GetWallet()!;
		Assert.Equal(2, wallet.AnonScoreTarget);
		Assert.Equal(isPrivate, wallet.IsWalletPrivate());
		Assert.Equal(isPrivate ? CoinJoinClientState.Idle : CoinJoinClientState.InProgress, fixture.Manager.Snapshot.State);
		if (isPrivate)
		{
			Assert.Equal(CoinjoinError.AllCoinsPrivate, fixture.Manager.Snapshot.WaitingReason);
			int attempts = 0;
			fixture.Manager.StatusChanged += (_, status) => { if (status is StartedEventArgs) { attempts++; } };
			await Task.Delay(1_200, TestContext.Current.CancellationToken);
			Assert.Equal(0, attempts);
			Assert.Equal(default, fixture.Manager.Snapshot.RetryAfter);
			fixture.Fund();
			await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.InProgress);
			Assert.Equal(1, attempts);
		}
	}

	[Fact]
	public async Task PrivateWalletStartsWhenPaymentArrivesAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: true, score: 2);
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.Equal(CoinJoinClientState.Idle, fixture.Manager.Snapshot.State);
		using var recipient = new Key();
		fixture.Application.Session.GetWallet()!.BatchedPayments.AddPayment(recipient.PubKey.GetAddress(ScriptPubKeyType.Segwit, Network.RegTest), Money.Coins(0.01m));
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.InProgress);
	}

	[Fact]
	public async Task BalanceSafeguardRequiresExplicitContinuationAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: true, amount: 0.001m);
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.Equal(CoinjoinError.NotEnoughUnprivateBalance, fixture.Manager.Snapshot.WaitingReason);
		fixture.Manager.RequestCoinJoinStart(overridePlebStop: true);
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.InProgress);
		Assert.True(fixture.Manager.Snapshot.OverridePlebStop);
		fixture.Manager.RequestCoinJoinStop();
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.Idle);
		fixture.Manager.RequestCoinJoinStart();
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.False(fixture.Manager.Snapshot.OverridePlebStop);
		Assert.Equal(CoinjoinError.NotEnoughUnprivateBalance, fixture.Manager.Snapshot.WaitingReason);
	}

	[Fact]
	public async Task CompletionStopsRoundWaitingWithoutPausingAsync()
	{
		await using var fixture = await Fixture.CreateAsync(funded: true);
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.Equal(CoinJoinClientState.InProgress, fixture.Manager.Snapshot.State);
		foreach (var coin in fixture.Application.Session.GetWallet()!.Coins) { coin.HdPubKey.SetAnonymitySet(2); }
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.WaitingReason == CoinjoinError.AllCoinsPrivate);
		Assert.Equal(CoinJoinClientState.Idle, fixture.Manager.Snapshot.State);
		Assert.False(fixture.Manager.Snapshot.IsPaused);
		Assert.Equal(default, fixture.Manager.Snapshot.RetryAfter);
		fixture.Fund();
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.State == CoinJoinClientState.InProgress);
	}

	[Fact]
	public async Task FailedRegistrationBacksOffWithoutRetryLoopAsync()
	{
		var clock = new ManualClock();
		var api = new FailingRegistrationApi();
		await using var fixture = await Fixture.CreateAsync(funded: true, api: api, clock: clock);
		api.NewRound();
		await fixture.PublishRoundAsync(api.RoundId);
		await SingleWalletTests.WaitForAsync(() => fixture.Manager.Snapshot.RetryAfter != default);
		Assert.Equal(clock.GetUtcNow() + TimeSpan.FromSeconds(30), fixture.Manager.Snapshot.RetryAfter);
		Assert.Equal(1, api.Registrations);
		clock.Advance(TimeSpan.FromSeconds(29));
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.Equal(1, api.Registrations);
		api.NewRound();
		await fixture.PublishRoundAsync(api.RoundId);
		clock.Advance(TimeSpan.FromSeconds(1));
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		try { await SingleWalletTests.WaitForAsync(() => api.Registrations == 2 && fixture.Manager.Snapshot.RetryAfter > clock.GetUtcNow()); }
		catch (OperationCanceledException)
		{
			Assert.Fail($"Attempts: {api.Registrations}; snapshot: {fixture.Manager.Snapshot}; clock: {clock.GetUtcNow():O}; coins: {string.Join(", ", fixture.Application.Session.GetWallet()!.Coins.Select(coin => $"confirmed={coin.Confirmed}, mixing={coin.CoinJoinInProgress}, score={coin.AnonymitySet}"))}");
		}
		await fixture.Manager.RestartAbortedCoinjoinsAsync();
		Assert.Equal(2, api.Registrations);
	}

	private sealed class ManualClock : TimeProvider
	{
		private DateTimeOffset _now = DateTimeOffset.UtcNow;
		public override DateTimeOffset GetUtcNow() => _now;
		public void Advance(TimeSpan elapsed) => _now += elapsed;
	}

	private sealed class FailingRegistrationApi : IWabiSabiApiRequestHandler
	{
		private RoundState? _round;
		public uint256 RoundId => _round!.Id;
		public int Registrations { get; private set; }
		public void NewRound() => _round = RoundState.FromRound(WabiSabiFactory.CreateRound(WabiSabiFactory.CreateRoundParameters(new WabiSabiConfig()) with
		{
			MinInputCountByRound = 21, MiningFeeRate = new FeeRate(1m), StandardInputRegistrationTimeout = TimeSpan.FromSeconds(65)
		}));
		public Task<RoundStateResponse> GetStatusAsync(RoundStateRequest request, CancellationToken cancel) => Task.FromResult(new RoundStateResponse(_round is { } round ? [round] : []));
		public Task<InputRegistrationResponse> RegisterInputAsync(InputRegistrationRequest request, CancellationToken cancel)
		{
			Registrations++;
			throw new WabiSabiProtocolException(WabiSabiProtocolErrorCode.RoundNotFound);
		}
		public Task<ConnectionConfirmationResponse> ConfirmConnectionAsync(ConnectionConfirmationRequest request, CancellationToken cancel) => throw new NotSupportedException();
		public Task RegisterOutputAsync(OutputRegistrationRequest request, CancellationToken cancel) => throw new NotSupportedException();
		public Task RemoveInputAsync(InputsRemovalRequest request, CancellationToken cancel) => throw new NotSupportedException();
		public Task<ReissueCredentialResponse> ReissuanceAsync(ReissueCredentialRequest request, CancellationToken cancel) => throw new NotSupportedException();
		public Task SignTransactionAsync(TransactionSignaturesRequest request, CancellationToken cancel) => throw new NotSupportedException();
		public Task ReadyToSignAsync(ReadyToSignRequestRequest request, CancellationToken cancel) => throw new NotSupportedException();
	}

	private sealed class Fixture : IAsyncDisposable
	{
		private Fixture(SingleWalletTests.SyntheticApplication application, CoinJoinManager manager, MagicalCryptoWallet.Services.MailboxProcessor<RoundUpdateMessage> rounds, CoinPrison prison)
		{ Application = application; Manager = manager; _rounds = rounds; _prison = prison; }
		private readonly MagicalCryptoWallet.Services.MailboxProcessor<RoundUpdateMessage> _rounds;
		private readonly CoinPrison _prison;
		public SingleWalletTests.SyntheticApplication Application { get; }
		public CoinJoinManager Manager { get; }
		public void Fund()
		{
			var wallet = Application.Session.GetWallet()!;
			var coin = BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(wallet.KeyManager), Money.Coins(0.1m), confirmed: true, anonymitySet: 1);
			wallet.TransactionProcessor.Process(coin.Transaction);
			wallet.TransactionStore.AddOrUpdate(coin.Transaction);
		}
		public async Task PublishRoundAsync(uint256 roundId)
		{
			using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(15));
			var published = new RoundStateProvider(_rounds).CreateRoundAwaiterAsync(round => round.Id == roundId, timeout.Token);
			_rounds.Post(new RoundUpdateMessage.UpdateMessage(DateTime.UtcNow));
			await published;
		}
		public static async Task<Fixture> CreateAsync(bool funded, string password = "", decimal amount = 0.1m, int score = 1, IWabiSabiApiRequestHandler? api = null, TimeProvider? clock = null)
		{
			var root = await Common.GetEmptyWorkDirAsync(callerMemberName: $"fixture-{Guid.NewGuid():N}");
			var app = new SingleWalletTests.SyntheticApplication(root);
			var keys = app.NewKeys(password);
			var wallet = app.Session.Configure(keys);
			if (funded)
			{
				foreach (var coin in ServiceFactory.CreateCoins(wallet.KeyManager, [("synthetic", 0, amount, true, score)]))
				{ wallet.TransactionProcessor.Process(coin.Transaction); wallet.TransactionStore.AddOrUpdate(coin.Transaction); coin.HdPubKey.SetAnonymitySet(score); }
			}
			await app.InitializeAsync();
			await SingleWalletTests.WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
			foreach (var coin in wallet.Coins) { coin.HdPubKey.SetAnonymitySet(score); }
			api ??= new WabiSabiHttpApiClient("synthetic", new MockHttpClientFactory());
			var rounds = RoundStateUpdaterForTesting.CreateManual(api);
			var prison = CoinPrison.CreateOrLoadFromFile(root);
			var manager = new CoinJoinManager(app.Session, new RoundStateProvider(rounds), _ => api,
				new CoinJoinConfiguration("synthetic", 50m), prison, app.Events, clock);
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
