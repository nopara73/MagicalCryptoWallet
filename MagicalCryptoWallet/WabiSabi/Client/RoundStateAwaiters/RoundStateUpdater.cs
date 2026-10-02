using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.WabiSabi.Coordinator.PostRequests;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.WabiSabi.Models;

namespace MagicalCryptoWallet.WabiSabi.Client.RoundStateAwaiters;


public abstract record RoundUpdateMessage
{
	public record UpdateMessage(DateTime CurrentTime) : RoundUpdateMessage;
	public record CreateRoundAwaiter(uint256? RoundId, Phase? Phase, Predicate<RoundState>? Predicate,
		IReplyChannel<Task<RoundState>> ReplayChannel, CancellationToken WaitCancellationToken = default) : RoundUpdateMessage;
}

public class RoundStateProvider(MailboxProcessor<RoundUpdateMessage> roundStateUpdater)
{
	public static readonly TimeSpan QueryFrequency = TimeSpan.FromSeconds(15);

	public async Task<RoundState> CreateRoundAwaiterAsync(uint256 roundId, Phase phase,
		CancellationToken cancellationToken)
	{
		using var cts = CancellationTokenSource.CreateLinkedTokenSource(roundStateUpdater.CancellationToken, cancellationToken);
		var awaiter = await roundStateUpdater
			.PostAndReplyAsync<Task<RoundState>>(
				chan => new RoundUpdateMessage.CreateRoundAwaiter(roundId, phase, null, chan, cts.Token),
				cts.Token).ConfigureAwait(false);
		return await awaiter.WaitAsync(cts.Token).ConfigureAwait(false);
	}

	public async Task<RoundState> CreateRoundAwaiterAsync(Predicate<RoundState> predicate,
		CancellationToken cancellationToken)
	{
		using var cts = CancellationTokenSource.CreateLinkedTokenSource(roundStateUpdater.CancellationToken, cancellationToken);
		var awaiter = await roundStateUpdater
			.PostAndReplyAsync<Task<RoundState>>(
				chan => new RoundUpdateMessage.CreateRoundAwaiter(null, null, predicate, chan, cts.Token),
				cts.Token).ConfigureAwait(false);
		return await awaiter.WaitAsync(cts.Token).ConfigureAwait(false);
	}
}

public record RoundsState(
	DateTime NextQueryTime,
	TimeSpan QueryInterval,
	Dictionary<uint256, RoundState> Rounds,
	ImmutableList<RoundStateAwaiter> Awaiters,
	int ConsecutiveFailures = 0);

public static class RoundStateUpdater
{
	public static MessageHandler<RoundUpdateMessage, RoundsState> Create(IWabiSabiApiRequestHandler arenaRequestHandler, TimeProvider? timeProvider = null) =>
		(msg, state, cancellationToken) => ProcessMessageAsync(msg, state, arenaRequestHandler, timeProvider ?? TimeProvider.System, cancellationToken);

	private static async Task<RoundsState> ProcessMessageAsync(
		RoundUpdateMessage msg,
		RoundsState state,
		IWabiSabiApiRequestHandler arenaRequestHandler,
		TimeProvider timeProvider,
		CancellationToken cancellationToken)
	{
		var finished = state.Awaiters.Where(a => a.Task.IsCompleted).ToArray();
		foreach (var awaiter in finished) { awaiter.Dispose(); }
		state = state with { Awaiters = state.Awaiters.RemoveRange(finished) };
		switch (msg)
		{
			case RoundUpdateMessage.UpdateMessage:
				if (state.Awaiters.Count > 0 && timeProvider.GetUtcNow().UtcDateTime >= state.NextQueryTime)
				{
					try
					{
						var (rounds, awaiters) = await UpdateRoundsStateAsync(state, arenaRequestHandler, cancellationToken).ConfigureAwait(false);
						state = state with
						{
							NextQueryTime = timeProvider.GetUtcNow().UtcDateTime + state.QueryInterval,
							Rounds = rounds,
							Awaiters = awaiters,
							ConsecutiveFailures = 0
						};
					}
					catch (Exception ex) when (!cancellationToken.IsCancellationRequested)
					{
						Logger.LogWarning(ex);
						var failures = Math.Min(state.ConsecutiveFailures + 1, 4);
						var delay = TimeSpan.FromSeconds(Math.Min(60, state.QueryInterval.TotalSeconds * Math.Pow(2, failures - 1)));
						state = state with { ConsecutiveFailures = failures, NextQueryTime = timeProvider.GetUtcNow().UtcDateTime + delay };
					}
				}

				break;
			case RoundUpdateMessage.CreateRoundAwaiter m:
				var roundStateAwaiter = new RoundStateAwaiter(m.Predicate, m.RoundId, m.Phase,
					m.WaitCancellationToken.CanBeCanceled ? m.WaitCancellationToken : cancellationToken);
				if (roundStateAwaiter.Task.IsCompleted ||
					(state.ConsecutiveFailures == 0 && (state.QueryInterval == TimeSpan.Zero || timeProvider.GetUtcNow().UtcDateTime < state.NextQueryTime) &&
					(m.RoundId is null || state.Rounds.ContainsKey(m.RoundId)) && roundStateAwaiter.IsCompleted(state.Rounds)))
				{
					roundStateAwaiter.Dispose();
				}
				else { state = state with { Awaiters = state.Awaiters.Add(roundStateAwaiter) }; }
				m.ReplayChannel.Reply(roundStateAwaiter.Task);
				break;
		}

		return state;
	}

	private static async Task<(Dictionary<uint256, RoundState> Rounds, ImmutableList<RoundStateAwaiter> Awaiters)> UpdateRoundsStateAsync(
		RoundsState state,
		IWabiSabiApiRequestHandler arenaRequestHandler,
		CancellationToken cancellationToken)
	{
		var request = new RoundStateRequest(
			state.Rounds.Select(x => new RoundStateCheckpoint(x.Key, x.Value.CoinjoinState.Events.Count)).ToImmutableList());

		using CancellationTokenSource timeoutCts = new(TimeSpan.FromSeconds(30));
		using var linkedCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, timeoutCts.Token);

		var response = await arenaRequestHandler.GetStatusAsync(request, linkedCts.Token).ConfigureAwait(false);
		RoundState[] roundStates = response.RoundStates;

		var updatedRoundStates = roundStates
			.Where(rs => state.Rounds.ContainsKey(rs.Id))
			.Select(rs => (NewRoundState: rs, CurrentRoundState: state.Rounds[rs.Id]))
			.Select(x => x.NewRoundState with { CoinjoinState = x.NewRoundState.CoinjoinState.AddPreviousStates(x.CurrentRoundState.CoinjoinState, x.NewRoundState.Id) })
			.ToList();

		var newRoundStates = roundStates
			.Where(rs => !state.Rounds.ContainsKey(rs.Id));

		if (newRoundStates.Any(r => !r.IsRoundIdMatching()))
		{
			throw new InvalidOperationException(
				"Coordinator is cheating by creating rounds that do not match the parameters.");
		}

		// Don't use ToImmutable dictionary, because that ruins the original order and makes the server unable to suggest a round preference.
		// ToDo: ToDictionary doesn't guarantee the order by design so .NET team might change this out of our feet, so there's room for improvement here.
		var finalRoundStates = newRoundStates.Concat(updatedRoundStates).ToDictionary(x => x.Id, x => x);

		var completedAwaiters = state.Awaiters.Where(awaiter => awaiter.IsCompleted(finalRoundStates)).ToArray();
		foreach (var awaiter in completedAwaiters) { awaiter.Dispose(); }
		return (Rounds: finalRoundStates, Awaiters: state.Awaiters.RemoveRange(completedAwaiters));
	}
}
