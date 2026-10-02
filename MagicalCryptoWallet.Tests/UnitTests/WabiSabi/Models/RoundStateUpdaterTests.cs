using System.Collections.Generic;
using System.Collections.Immutable;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.WabiSabi.Client.RoundStateAwaiters;
using MagicalCryptoWallet.WabiSabi.Models;
using Xunit;
using MagicalCryptoWallet.Serialization;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Tests.UnitTests.Services;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using static MagicalCryptoWallet.Services.Workers;
using MagicalCryptoWallet.Tests.UnitTests.Mocks;

namespace MagicalCryptoWallet.Tests.UnitTests.WabiSabi.Models;

public class RoundStateUpdaterTests
{
	[Fact]
	public async Task CancelledAwaitersAreRemovedBeforeSendingStatusRequestsAsync()
	{
		var requests = 0;
		var clock = new ManualTimeProvider();
		var api = new WabiSabiHttpApiClient("synthetic", MockHttpClientFactory.Create(() =>
		{
			requests++;
			return RoundStateResponseBuilder()();
		}));
		var handler = RoundStateUpdater.Create(api, clock);
		var state = new RoundsState(clock.GetUtcNow().UtcDateTime, TimeSpan.FromSeconds(15), [], []);
		using var cancellation = new CancellationTokenSource();
		var reply = new TestReplyChannel<Task<RoundState>>();
		state = await handler(new RoundUpdateMessage.CreateRoundAwaiter(null, null, _ => false, reply, cancellation.Token), state, CancellationToken.None);
		await cancellation.CancelAsync();
		state = await handler(new RoundUpdateMessage.UpdateMessage(clock.GetUtcNow().UtcDateTime), state, CancellationToken.None);
		Assert.True(reply.Result!.IsCanceled);
		Assert.Empty(state.Awaiters);
		Assert.Equal(0, requests);
	}

	[Fact]
	public async Task ProviderCarriesCancellationIntoRegisteredAwaiterAsync()
	{
		var observed = new TaskCompletionSource<CancellationToken>(TaskCreationOptions.RunContinuationsAsynchronously);
		var api = new WabiSabiHttpApiClient("synthetic", MockHttpClientFactory.Create(RoundStateResponseBuilder()));
		var handler = RoundStateUpdater.Create(api);
		using var worker = Spawn($"round-cancellation-{Guid.NewGuid()}", EventDriven(
			new RoundsState(DateTime.UtcNow, TimeSpan.FromSeconds(15), [], []),
			async (RoundUpdateMessage msg, RoundsState state, CancellationToken token) =>
			{
				var next = await handler(msg, state, token);
				if (msg is RoundUpdateMessage.CreateRoundAwaiter m) { observed.TrySetResult(m.WaitCancellationToken); }
				return next;
			}));
		using var cancellation = new CancellationTokenSource(TimeSpan.FromSeconds(10));
		var waiting = new RoundStateProvider(worker).CreateRoundAwaiterAsync(_ => false, cancellation.Token);
		var waiterToken = await observed.Task.WaitAsync(cancellation.Token);
		await cancellation.CancelAsync();
		await Assert.ThrowsAnyAsync<OperationCanceledException>(() => waiting);
		Assert.True(waiterToken.IsCancellationRequested);
	}

	[Fact]
	public async Task StatusFailuresBackOffAndSuccessfulPollUsesCompletionTimeAsync()
	{
		var clock = new ManualTimeProvider();
		var requests = 0;
		var succeeds = false;
		var api = new WabiSabiHttpApiClient("synthetic", new MockHttpClientFactory
		{
			OnCreateClient = _ => new MockHttpClient
			{
				OnSendAsync = _ =>
				{
					requests++;
					if (!succeeds) { throw new HttpRequestException("synthetic failure"); }
					clock.Advance(TimeSpan.FromSeconds(7));
					return Task.FromResult(RoundStateResponseBuilder()());
				}
			}
		});
		var handler = RoundStateUpdater.Create(api, clock);
		using var awaiter = new RoundStateAwaiter(_ => false, null, null, CancellationToken.None);
		var state = new RoundsState(clock.GetUtcNow().UtcDateTime, TimeSpan.FromSeconds(15), [], [awaiter]);
		async Task TickAsync() => state = await handler(new RoundUpdateMessage.UpdateMessage(DateTime.MinValue), state, CancellationToken.None);
		await TickAsync();
		for (var n = 0; n < 10; n++) { await TickAsync(); }
		Assert.Equal(1, requests);
		clock.Advance(TimeSpan.FromSeconds(15));
		await TickAsync();
		Assert.Equal(2, requests);
		Assert.Equal(clock.GetUtcNow().UtcDateTime.AddSeconds(30), state.NextQueryTime);
		clock.Advance(TimeSpan.FromSeconds(30));
		await TickAsync();
		Assert.Equal(clock.GetUtcNow().UtcDateTime.AddSeconds(60), state.NextQueryTime);
		clock.Advance(TimeSpan.FromSeconds(60));
		succeeds = true;
		await TickAsync();
		Assert.Equal(0, state.ConsecutiveFailures);
		Assert.Equal(clock.GetUtcNow().UtcDateTime.AddSeconds(15), state.NextQueryTime);
		await TickAsync();
		Assert.Equal(4, requests);
	}

	[Fact]
	public async Task MatchingCachedRoundCompletesWithoutPollingAsync()
	{
		var round = RoundState.FromRound(WabiSabiFactory.CreateRound(cfg: new()));
		var api = new WabiSabiHttpApiClient("synthetic", MockHttpClientFactory.Create(() => throw new InvalidOperationException("Unexpected request")));
		var handler = RoundStateUpdater.Create(api);
		var state = new RoundsState(DateTime.UtcNow.AddSeconds(15), TimeSpan.FromSeconds(15), new Dictionary<uint256, RoundState> { [round.Id] = round }, []);
		var reply = new TestReplyChannel<Task<RoundState>>();
		state = await handler(new RoundUpdateMessage.CreateRoundAwaiter(round.Id, round.Phase, null, reply), state, CancellationToken.None);
		Assert.Same(round, await reply.Result!);
		Assert.Empty(state.Awaiters);
	}

	[Theory]
	[InlineData(0)]
	[InlineData(1)]
	public async Task StaleOrFailedStatusRequiresRefreshBeforeCompletingNewWaitersAsync(int failures)
	{
		var round = RoundState.FromRound(WabiSabiFactory.CreateRound(cfg: new()));
		var clock = new ManualTimeProvider();
		var api = new WabiSabiHttpApiClient("synthetic", MockHttpClientFactory.Create(RoundStateResponseBuilder()));
		var handler = RoundStateUpdater.Create(api, clock);
		var nextQuery = clock.GetUtcNow().UtcDateTime.AddSeconds(failures == 0 ? -1 : 30);
		var state = new RoundsState(nextQuery, TimeSpan.FromSeconds(15), new Dictionary<uint256, RoundState> { [round.Id] = round }, [], failures);
		var reply = new TestReplyChannel<Task<RoundState>>();
		state = await handler(new RoundUpdateMessage.CreateRoundAwaiter(round.Id, round.Phase, null, reply), state, CancellationToken.None);
		Assert.False(reply.Result!.IsCompleted);
		Assert.Single(state.Awaiters);
		clock.Advance(TimeSpan.FromSeconds(31));
		state = await handler(new RoundUpdateMessage.UpdateMessage(clock.GetUtcNow().UtcDateTime), state, CancellationToken.None);
		await Assert.ThrowsAsync<InvalidOperationException>(() => reply.Result);
		Assert.Empty(state.Awaiters);
	}

	private static readonly TimeSpan TestTimeOut = TimeSpan.FromMinutes(10);

	[Fact]
	public async Task UpdatesAutomaticallyAsync()
	{
		var roundState = RoundState.FromRound(WabiSabiFactory.CreateRound(cfg: new()));
		var mockHttpClientFactory = MockHttpClientFactory.Create([
			RoundStateResponseBuilder(roundState with { Phase = Phase.InputRegistration }),
			RoundStateResponseBuilder(roundState with { Phase = Phase.OutputRegistration, CoinjoinState = roundState.CoinjoinState.GetStateFrom(1) })
		]);
		var apiClient = new WabiSabiHttpApiClient("identity", mockHttpClientFactory);
		using var cts = new CancellationTokenSource(TimeSpan.FromSeconds(30));
		using var roundStatusUpdater = RoundStateUpdaterForTesting.Create(apiClient, cts.Token);
		var roundStatusProvider = new RoundStateProvider(roundStatusUpdater);

		await roundStatusProvider.CreateRoundAwaiterAsync(roundState.Id, Phase.InputRegistration, cts.Token);
		await roundStatusProvider.CreateRoundAwaiterAsync(roundState.Id, Phase.OutputRegistration, cts.Token);
	}

	[Fact]
	public async Task RoundStateUpdaterTestsAsync()
	{
		var roundState1 = RoundState.FromRound(WabiSabiFactory.CreateRound(cfg: new()));
		var roundState2 = RoundState.FromRound(WabiSabiFactory.CreateRound(cfg: new()));

		using CancellationTokenSource cancellationTokenSource = new(TestTimeOut);
		var cancellationToken = cancellationTokenSource.Token;

		// The coordinator creates two rounds.
		// Each line represents a response for each request.
		var mockHttpClientFactory = MockHttpClientFactory.Create([
			RoundStateResponseBuilder(roundState1 with { Phase = Phase.InputRegistration }),
			RoundStateResponseBuilder(roundState1 with { Phase = Phase.OutputRegistration, CoinjoinState = roundState1.CoinjoinState.GetStateFrom(1)}),
			RoundStateResponseBuilder(roundState1 with { Phase = Phase.OutputRegistration, CoinjoinState = roundState1.CoinjoinState.GetStateFrom(2)}, roundState2 with { Phase = Phase.InputRegistration }),
			RoundStateResponseBuilder(roundState2 with { Phase = Phase.OutputRegistration }),
			RoundStateResponseBuilder()
		]);
		var apiClient = new WabiSabiHttpApiClient("identity", mockHttpClientFactory);

		using var roundStatusUpdaterCancellation = new CancellationTokenSource();
		using var roundStatusUpdater = RoundStateUpdaterForTesting.CreateManual(apiClient, roundStatusUpdaterCancellation.Token);
		var roundStatusProvider = new RoundStateProvider(roundStatusUpdater);

		// At this point in time the RoundStateUpdater only knows about `round1` and then we can subscribe to
		// events for that round.
		using var round1TSCts = new CancellationTokenSource();
		var round1IRTask = roundStatusProvider.CreateRoundAwaiterAsync(roundState1.Id, Phase.InputRegistration, cancellationToken);
		var round1ORTask = roundStatusProvider.CreateRoundAwaiterAsync(roundState1.Id, Phase.OutputRegistration, cancellationToken);
		var round1TSTask = roundStatusProvider.CreateRoundAwaiterAsync(roundState1.Id, Phase.TransactionSigning, round1TSCts.Token);
		var round1TBTask = roundStatusProvider.CreateRoundAwaiterAsync(roundState1.Id, Phase.Ended, cancellationToken);

		await Task.Delay(TimeSpan.FromMilliseconds(100)).ContinueWith(_ => roundStatusUpdater.Update());

		// Wait for round1 in input registration.
		var round1 = await round1IRTask;
		Assert.Equal(roundState1.Id, round1.Id);
		Assert.Equal(Phase.InputRegistration, round1.Phase);
		Assert.All(new[] { round1ORTask, round1TSTask, round1TBTask }, t => Assert.Equal(TaskStatus.WaitingForActivation, t.Status));

		// Force the RoundStatusUpdater to run. After this it will know about the existence of `round2` so,
		// we can subscribe to events.
		roundStatusUpdater.Update();
		var round2IRTask = roundStatusProvider.CreateRoundAwaiterAsync(roundState2.Id, Phase.InputRegistration, cancellationToken);
		var round2TBTask = roundStatusProvider.CreateRoundAwaiterAsync(roundState2.Id, Phase.Ended, cancellationToken);

		// Force the RoundStatusUpdater to run again just to make it trigger the events.
		roundStatusUpdater.Update();

		// Wait for round1 in input registration.
		var round2 = await round2IRTask;
		Assert.Equal(roundState2.Id, round2.Id);
		Assert.Equal(Phase.InputRegistration, round2.Phase);
		Assert.All(new[] { round1TSTask, round1TBTask, round2TBTask }, t => Assert.Equal(TaskStatus.WaitingForActivation, t.Status));

		// `round1` changed to output registration phase even before `round2` was created so, it has to be completed.
		round1 = await round1ORTask;
		Assert.Equal(roundState1.Id, round1.Id);
		Assert.Equal(Phase.OutputRegistration, round1.Phase);
		Assert.All(new[] { round1TSTask, round1TBTask, round2TBTask }, t => Assert.Equal(TaskStatus.WaitingForActivation, t.Status));

		// We cancel the cancellation token source used for the `wake me up when round1 transactions has to be signed` awaiter
		await round1TSCts.CancelAsync();
		Assert.True(round1TSTask.IsCanceled);

		// At this point in time all the rounds have disappeared and then the awaiter that was waiting for round1 to broadcast
		// the transaction has to fail to let the sleeping component that the round doesn't exist any more.
		roundStatusUpdater.Update();
		var ex = await Assert.ThrowsAsync<InvalidOperationException>(async () => await round1TBTask);
		Assert.Contains(round1.Id.ToString(), ex.Message);
		Assert.Contains("not running", ex.Message);

		// `Round2` awaiter has to be cancelled immediately when we stop the updater.
		Assert.Equal(TaskStatus.WaitingForActivation, round2TBTask.Status);
		await roundStatusUpdaterCancellation.CancelAsync();

		Assert.Equal(TaskStatus.Canceled, round2TBTask.Status);
	}

	[Fact]
	public async Task RoundStateUpdaterFailureRecoveryTestsAsync()
	{
		var roundState = RoundState.FromRound(WabiSabiFactory.CreateRound(cfg: new()));

		using var cancellationTokenSource = new CancellationTokenSource();
		var cancellationToken = cancellationTokenSource.Token;

		// Each line represents a response for each request.
		// Exceptions, Problems, Errors everywhere!!!
		var mockHttpClientFactory = MockHttpClientFactory.Create([
			RoundStateResponseBuilder(roundState with {Phase = Phase.InputRegistration}),
			() => throw new Exception(),
			() => throw new OperationCanceledException(),
			() => throw new InvalidOperationException(),
			() => throw new HttpRequestException(),
			RoundStateResponseBuilder(roundState with {Phase = Phase.OutputRegistration}),
			RoundStateResponseBuilder()
		]);
		var apiClient = new WabiSabiHttpApiClient("identity", mockHttpClientFactory);

		using var roundStatusUpdater = RoundStateUpdaterForTesting.CreateManual(apiClient);
		var roundStatusProvider = new RoundStateProvider(roundStatusUpdater);

		// At this point in time the RoundStateUpdater only knows about `round1` and then we can subscribe to
		// events for that round.
		using var roundTSCts = new CancellationTokenSource();
		var roundIRTask = roundStatusProvider.CreateRoundAwaiterAsync(roundState.Id, Phase.InputRegistration, cancellationToken);
		var roundORTask = roundStatusProvider.CreateRoundAwaiterAsync(roundState.Id, Phase.OutputRegistration, cancellationToken);

		roundStatusUpdater.Update();
		// Wait for round1 in input registration.
		var round = await roundIRTask;
		Assert.Equal(Phase.InputRegistration, round.Phase);
		Assert.Equal(TaskStatus.WaitingForActivation, roundORTask.Status);

		// Force the RoundStatusUpdater to run again just to make it trigger the events.
		// Lots of exceptions in the meanwhile
		roundStatusUpdater.Update();
		roundStatusUpdater.Update();
		roundStatusUpdater.Update();
		roundStatusUpdater.Update();
		roundStatusUpdater.Update();
		await Task.Delay(TimeSpan.FromSeconds(1));

		// But in the end everything is alright.
		round = await roundORTask;
		Assert.Equal(Phase.OutputRegistration, round.Phase);
	}

	[Fact]
	public async Task FailOnUnexpectedAsync()
	{
		var roundState = RoundState.FromRound(WabiSabiFactory.CreateRound(cfg: new()));

		using var cancellationTokenSource = new CancellationTokenSource();
		var cancellationToken = cancellationTokenSource.Token;

		// Each line represents a response for each request.
		// Exceptions, Problems, Errors everywhere!!!
		var mockHttpClientFactory = MockHttpClientFactory.Create([
			RoundStateResponseBuilder(roundState with {Phase = Phase.InputRegistration}),
			() => throw new Exception(),
			() => throw new OperationCanceledException(),
			() => throw new InvalidOperationException(),
			() => throw new HttpRequestException(),
			RoundStateResponseBuilder(roundState with {Phase = Phase.Ended}),
			RoundStateResponseBuilder()
		]);
		var apiClient = new WabiSabiHttpApiClient("identity", mockHttpClientFactory);

		using var roundStatusUpdater = RoundStateUpdaterForTesting.CreateManual(apiClient);
		var roundStatusProvider = new RoundStateProvider(roundStatusUpdater);

		// At this point in time the RoundStateUpdater only knows about `round1` and then we can subscribe to
		// events for that round.
		using var roundTSCts = new CancellationTokenSource();
		var roundIRTask = roundStatusProvider.CreateRoundAwaiterAsync(roundState.Id, Phase.InputRegistration, cancellationToken);
		var roundORTask = roundStatusProvider.CreateRoundAwaiterAsync(roundState.Id, Phase.OutputRegistration, cancellationToken);

		roundStatusUpdater.Update();
		// Wait for round1 in input registration.
		var round = await roundIRTask;
		Assert.Equal(Phase.InputRegistration, round.Phase);
		Assert.Equal(TaskStatus.WaitingForActivation, roundORTask.Status);

		// Force the RoundStatusUpdater to run again just to make it trigger the events.
		// Lots of exceptions in the meanwhile
		roundStatusUpdater.Update();
		roundStatusUpdater.Update();
		roundStatusUpdater.Update();
		roundStatusUpdater.Update();
		roundStatusUpdater.Update();

		// We are expecting output registration phase but the round unexpectedly ends.
		await Assert.ThrowsAsync<UnexpectedRoundPhaseException>(async () => await roundORTask);
	}

	[Fact]
	public async Task CancelAsync()
	{
		var roundState = RoundState.FromRound(WabiSabiFactory.CreateRound(cfg: new()));

		var mockHttpClientFactory = MockHttpClientFactory.Create([
			RoundStateResponseBuilder(roundState with {Phase = Phase.InputRegistration})
		]);
		var apiClient = new WabiSabiHttpApiClient("identity", mockHttpClientFactory);

		using var roundStatusUpdater = RoundStateUpdaterForTesting.CreateManual(apiClient);
		var roundStatusProvider = new RoundStateProvider(roundStatusUpdater);
		using var cancellationTokenSource = new CancellationTokenSource(TimeSpan.FromSeconds(1));

		await Assert.ThrowsAsync<TaskCanceledException>(async () =>
			await roundStatusProvider.CreateRoundAwaiterAsync(uint256.One, Phase.InputRegistration, cancellationTokenSource.Token));
	}

	private static Func<HttpResponseMessage> RoundStateResponseBuilder(params RoundState[] roundStates) =>
		() => HttpResponseMessageEx.Ok(
			Encode.RoundStateResponse( new RoundStateResponse(roundStates)).ToJsonString());
}

public static class RoundStateUpdaterExtensions
{
	public static void Update(this MailboxProcessor<RoundUpdateMessage> roundStateUpdater) =>
		roundStateUpdater.Post(new RoundUpdateMessage.UpdateMessage(DateTime.UtcNow));
}
