using System.Diagnostics;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;

public class CoinJoinCoinSelector
{
	public const int MaxInputsRegistrableByWallet = 10;
	public const int MaxWeightedAnonLoss = 3; // Maximum tolerable WeightedAnonLoss.

	public CoinJoinCoinSelector(
		CoinJoinCoinSelectorRandomnessGenerator? generator = null,
		Func<bool>? arePaymentsPending = null)
	{
		_generator = generator ?? new(RandomnessProviders.Secure);
		_arePaymentsPending = arePaymentsPending ?? (() => false);
	}

	private RandomnessProvider Rnd => _generator.Rnd;
	private readonly CoinJoinCoinSelectorRandomnessGenerator _generator;
	private readonly Func<bool> _arePaymentsPending;

	public static CoinJoinCoinSelector FromWallet(Wallet wallet) =>
		new(arePaymentsPending: () => wallet.BatchedPayments.AreTherePendingPayments);

	/// <param name="liquidityClue">Weakly prefer not to select inputs over this.</param>
	public ImmutableList<SmartCoin> SelectCoinsForRound(IEnumerable<SmartCoin> coins, UtxoSelectionParameters parameters, Money liquidityClue)
	{
		liquidityClue = liquidityClue > Money.Zero
			? liquidityClue
			: Constants.MaximumNumberOfBitcoinsMoney;

		var filteredCoins = coins
			.Where(x => parameters.AllowedInputAmounts.Contains(x.Amount))
			.Where(x => parameters.AllowedInputScriptTypes.Contains(x.ScriptType))
			.Where(x => x.EffectiveValue(parameters.MiningFeeRate) > Money.Zero)
			.ToArray();

		// Sanity check.
		if (filteredCoins.Length == 0)
		{
			Logger.LogDebug("No suitable coins for this round.");
			return ImmutableList<SmartCoin>.Empty;
		}

		var privateCoins = filteredCoins.Where(x => x.IsPrivate(Constants.AnonymityScoreTarget)).ToArray();
		var allowedNonPrivateCoins = filteredCoins.Where(x => !x.IsPrivate(Constants.AnonymityScoreTarget)).ToList();
		// A queued payment can use already-private funds even when there is nothing left to mix.
		if (allowedNonPrivateCoins.Count == 0 && _arePaymentsPending())
		{
			allowedNonPrivateCoins.AddRange(privateCoins);
			privateCoins = [];
		}
		if (allowedNonPrivateCoins.Count == 0)
		{
			return ImmutableList<SmartCoin>.Empty;
		}

		int inputCount = Math.Min(privateCoins.Length + allowedNonPrivateCoins.Count, MaxInputsRegistrableByWallet);
		var biasShuffledPrivateCoins = AnonScoreTxSourceBiasedShuffle(privateCoins).ToArray();

		// Deprioritize private coins those are too large.
		var smallerPrivateCoins = biasShuffledPrivateCoins.Where(x => x.Amount <= liquidityClue);
		var largerPrivateCoins = biasShuffledPrivateCoins.Where(x => x.Amount > liquidityClue);

		// Let's allow only inputCount - 1 private coins to play.
		var allowedPrivateCoins = smallerPrivateCoins.Concat(largerPrivateCoins).Take(inputCount - 1).ToArray();
		Logger.LogDebug($"{nameof(allowedPrivateCoins)}: {allowedPrivateCoins.Length} coins, valued at {Money.Satoshis(allowedPrivateCoins.Sum(x => x.Amount)).ToString(false, true)} BTC.");

		var allowedCoins = allowedNonPrivateCoins.Concat(allowedPrivateCoins).ToArray();
		Logger.LogDebug($"{nameof(allowedCoins)}: {allowedCoins.Length} coins, valued at {Money.Satoshis(allowedCoins.Sum(x => x.Amount)).ToString(false, true)} BTC.");

		// Shuffle coins, while randomly biasing towards lower AS.
		var orderedAllowedCoins = AnonScoreTxSourceBiasedShuffle(allowedCoins).ToArray();

		// Always use the largest amounts, so we do not participate with insignificant amounts and fragment wallet needlessly.
		var largestNonPrivateCoins = allowedNonPrivateCoins
			.OrderByDescending(x => x.Amount)
			.Take(3)
			.ToArray();
		Logger.LogDebug($"Largest non-private coins: {string.Join(", ", largestNonPrivateCoins.Select(x => x.Amount.ToString(false, true)).ToArray())} BTC.");

		// Select a group of coins those are close to each other by anonymity score.
		Dictionary<int, IEnumerable<SmartCoin>> groups = new();

		// Create a bunch of combinations.
		var sw1 = Stopwatch.StartNew();
		foreach (var coin in largestNonPrivateCoins)
		{
			// Create a base combination just in case.
			var baseGroup = orderedAllowedCoins.Except(new[] { coin }).Take(inputCount - 1).Concat(new[] { coin });
			TryAddGroup(parameters, groups, baseGroup);

			var sw2 = Stopwatch.StartNew();
			foreach (var group in orderedAllowedCoins
				.Except(new[] { coin })
				.CombinationsWithoutRepetition(inputCount - 1)
				.Select(x => x.Concat(new[] { coin })))
			{
				TryAddGroup(parameters, groups, group);

				if (sw2.Elapsed > TimeSpan.FromSeconds(1))
				{
					break;
				}
			}

			sw2.Reset();

			if (sw1.Elapsed > TimeSpan.FromSeconds(10))
			{
				break;
			}
		}

		if (groups.Count == 0)
		{
			Logger.LogDebug($"Couldn't create any combinations, ending.");
			return ImmutableList<SmartCoin>.Empty;
		}
		Logger.LogDebug($"Created {groups.Count} combinations within {(int)sw1.Elapsed.TotalSeconds} seconds.");

		// Select the group where the less coins coming from the same tx.
		var bestRep = groups.Values.Select(x => GetReps(x)).Min(x => x);
		var bestRepGroups = groups.Values.Where(x => GetReps(x) == bestRep);
		Logger.LogDebug($"{nameof(bestRep)}: {bestRep}.");
		Logger.LogDebug($"Filtered combinations down to {nameof(bestRepGroups)}: {bestRepGroups.Count()}.");

		var remainingLargestNonPrivateCoins = largestNonPrivateCoins.Where(x => bestRepGroups.Any(y => y.Contains(x)));
		Logger.LogDebug($"Remaining largest non-private coins: {string.Join(", ", remainingLargestNonPrivateCoins.Select(x => x.Amount.ToString(false, true)).ToArray())} BTC.");

		// Bias selection towards larger numbers.
		var selectedNonPrivateCoin = remainingLargestNonPrivateCoins.RandomElement(Rnd); // Select randomly at first just to have a starting value.
		foreach (var coin in remainingLargestNonPrivateCoins.OrderByDescending(x => x.Amount))
		{
			if (Rnd.GetInt(100) < 50)
			{
				selectedNonPrivateCoin = coin;
				break;
			}
		}
		if (selectedNonPrivateCoin is null)
		{
			Logger.LogDebug($"Couldn't select largest non-private coin, ending.");
			return ImmutableList<SmartCoin>.Empty;
		}
		Logger.LogDebug($"Randomly selected large non-private coin: {selectedNonPrivateCoin.Amount.ToString(false, true)}.");

		var finalCandidate = bestRepGroups
			.Where(x => x.Contains(selectedNonPrivateCoin))
			.RandomElement(Rnd);
		if (finalCandidate is null)
		{
			Logger.LogDebug($"Couldn't select final selection candidate, ending.");
			return ImmutableList<SmartCoin>.Empty;
		}
		Logger.LogDebug($"Selected the final selection candidate: {finalCandidate.Count()} coins, {string.Join(", ", finalCandidate.Select(x => x.Amount.ToString(false, true)).ToArray())} BTC.");

		// Let's remove some coins coming from the same tx in the final candidate, allow 2 on average.
		int sameTxAllowance = _generator.GetRandomBiasedSameTxAllowance(67);

		List<SmartCoin> winner = new()
		{
			selectedNonPrivateCoin
		};

		foreach (var coin in finalCandidate
			.Except(new[] { selectedNonPrivateCoin })
			.OrderBy(x => x.AnonymitySet)
			.ThenByDescending(x => x.Amount))
		{
			// If the coin is coming from same tx, then check our allowance.
			if (winner.Any(x => x.TransactionId == coin.TransactionId))
			{
				var sameTxUsed = winner.Count - winner.Select(x => x.TransactionId).Distinct().Count();
				if (sameTxUsed < sameTxAllowance)
				{
					winner.Add(coin);
				}
			}
			else
			{
				winner.Add(coin);
			}
		}

		double winnerAnonLoss = GetAnonLoss(winner);

		// Only stay in the while if we are above the liquidityClue (we are a whale) AND the weightedAnonLoss is not tolerable.
		while (winner.Sum(x => x.Amount) > liquidityClue && winnerAnonLoss > MaxWeightedAnonLoss)
		{
			List<SmartCoin> bestReducedWinner = winner;
			var bestAnonLoss = winnerAnonLoss;
			bool winnerChanged = false;

			// We always want to keep the non-private coins.
			foreach (SmartCoin coin in winner.Except(new[] { selectedNonPrivateCoin }))
			{
				var reducedWinner = winner.Except(new[] { coin });
				var anonLoss = GetAnonLoss(reducedWinner);

				if (anonLoss <= bestAnonLoss)
				{
					bestAnonLoss = anonLoss;
					bestReducedWinner = reducedWinner.ToList();
					winnerChanged = true;
				}
			}

			if (!winnerChanged)
			{
				break;
			}

			winner = bestReducedWinner;
			winnerAnonLoss = bestAnonLoss;
		}

		if (winner.Count != finalCandidate.Count())
		{
			Logger.LogDebug($"Optimizing selection, removing coins coming from the same tx.");
			Logger.LogDebug($"{nameof(sameTxAllowance)}: {sameTxAllowance}.");
			Logger.LogDebug($"{nameof(winner)}: {winner.Count} coins, {string.Join(", ", winner.Select(x => x.Amount.ToString(false, true)).ToArray())} BTC.");
		}

		if (winner.Count < MaxInputsRegistrableByWallet)
		{
			// If the address of a winner contains other coins (address reuse, same HdPubKey) that are available but not selected,
			// complete the selection with them until MaxInputsRegistrableByWallet threshold.
			// Order by most to least reused to try not splitting coins from same address into several rounds.
			var nonSelectedCoinsOnSameAddresses = filteredCoins
				.Except(winner)
				.Where(x => winner.Any(y => y.ScriptPubKey == x.ScriptPubKey))
				.GroupBy(x => x.ScriptPubKey)
				.OrderByDescending(g => g.Count())
				.SelectMany(g => g)
				.Take(MaxInputsRegistrableByWallet - winner.Count)
				.ToList();

			winner.AddRange(nonSelectedCoinsOnSameAddresses);

			if (nonSelectedCoinsOnSameAddresses.Count > 0)
			{
				Logger.LogInfo($"{nonSelectedCoinsOnSameAddresses.Count} coins were added to the selection because they are on the same addresses of some selected coins.");
			}
		}

		// Privacy pruning can reduce an initially viable batch below the output minimum.
		if (winner.Sum(x => x.EffectiveValue(parameters.MiningFeeRate)) < parameters.MinAllowedOutputAmount)
		{
			return ImmutableList<SmartCoin>.Empty;
		}
		return winner.ToShuffled(Rnd).ToImmutableList();
	}

	private IEnumerable<SmartCoin> AnonScoreTxSourceBiasedShuffle(SmartCoin[] coins)
	{
		var orderedCoins = new List<SmartCoin>();
		for (int i = 0; i < coins.Length; i++)
		{
			// Order by anonscore first.
			var remaining = coins.Except(orderedCoins).OrderBy(x => x.AnonymitySet);

			// Then manipulate the list so repeating tx sources go to the end.
			var alternating = new List<SmartCoin>();
			var skipped = new List<SmartCoin>();
			foreach (var c in remaining)
			{
				if (alternating.Any(x => x.TransactionId == c.TransactionId) || orderedCoins.Any(x => x.TransactionId == c.TransactionId))
				{
					skipped.Add(c);
				}
				else
				{
					alternating.Add(c);
				}
			}
			alternating.AddRange(skipped);

			var coin = alternating.BiasedRandomElement(biasPercent: 50, Rnd)!;
			orderedCoins.Add(coin);
			yield return coin;
		}
	}

	private static bool TryAddGroup(UtxoSelectionParameters parameters, Dictionary<int, IEnumerable<SmartCoin>> groups, IEnumerable<SmartCoin> group)
	{
		var effectiveInputSum = group.Sum(x => x.EffectiveValue(parameters.MiningFeeRate));
		if (effectiveInputSum >= parameters.MinAllowedOutputAmount)
		{
			var k = HashCode.Combine(group.OrderBy(x => x.TransactionId).ThenBy(x => x.Index));
			return groups.TryAdd(k, group);
		}

		return false;
	}

	private static double GetAnonLoss(IEnumerable<SmartCoin> coins)
	{
		double minimumAnonScore = coins.Min(x => x.AnonymitySet);
		return coins.Sum(x => (x.AnonymitySet - minimumAnonScore) * x.Amount.Satoshi) / coins.Sum(x => x.Amount.Satoshi);
	}

	private static int GetReps(IEnumerable<SmartCoin> group)
		=> group.GroupBy(x => x.TransactionId).Sum(coinsInTxGroup => coinsInTxGroup.Count() - 1);
}
