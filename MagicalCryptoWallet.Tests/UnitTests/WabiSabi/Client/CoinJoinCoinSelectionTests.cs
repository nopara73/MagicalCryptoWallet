using System.Linq;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.WabiSabi.Client;

/// <summary>
/// Tests for <see cref="CoinJoinCoinSelector"/>.
/// </summary>
public class CoinJoinCoinSelectionTests
{
	[Theory]
	[InlineData(1.0, true)]
	[InlineData(1.99, true)]
	[InlineData(2.0, false)]
	[InlineData(3.0, false)]
	public void FixedTargetIncludesOnlyBelowTwoWithoutPayments(double score, bool selected)
	{
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coin = BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(1m));
		coin.HdPubKey.SetAnonymitySet(score);
		var result = new CoinJoinCoinSelector(CreateSelectorGenerator()).SelectCoinsForRound([coin], CreateUtxoSelectionParameters(), Constants.MaximumNumberOfBitcoinsMoney);
		Assert.Equal(selected ? 1 : 0, result.Count);
	}

	[Theory]
	[InlineData(3)]
	[InlineData(10)]
	[InlineData(15)]
	public void BatchesUnmixedCoinsUpToTenInputs(int count)
	{
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coins = Enumerable.Range(0, count).Select(i => BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(1m), anonymitySet: 1)).ToArray();
		var result = new CoinJoinCoinSelector(CreateSelectorGenerator(sameTxAllowance: 0)).SelectCoinsForRound(coins, CreateUtxoSelectionParameters(), Constants.MaximumNumberOfBitcoinsMoney);
		Assert.Equal(Math.Min(count, 10), result.Count);
		Assert.All(result, coin => Assert.Contains(coin, coins));
	}

	[Fact]
	public void PrivacyPruningCannotLeaveAnUneconomicalBatch()
	{
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var keys = new[] { BitcoinFactory.CreateHdPubKey(km), BitcoinFactory.CreateHdPubKey(km) };
		var tx = Transaction.Create(Network.Main);
		tx.Inputs.Add(BitcoinFactory.CreateOutPoint());
		foreach (var key in keys) { tx.Outputs.Add(new TxOut(Money.Satoshis(10_000), key.GetAssumedScriptPubKey())); }
		var transaction = new SmartTransaction(tx, new Height.ChainHeight(100));
		var coins = keys.Select((key, i) => new SmartCoin(transaction, (uint)i, key)).ToArray();
		var parameters = CreateUtxoSelectionParameters() with { MinAllowedOutputAmount = Money.Satoshis(15_000), MiningFeeRate = new FeeRate(1m) };
		var selector = new CoinJoinCoinSelector(CreateSelectorGenerator(sameTxAllowance: 0));
		Assert.Empty(selector.SelectCoinsForRound(coins, parameters, Constants.MaximumNumberOfBitcoinsMoney));
	}
	/// <summary>
	/// This test is to make sure no coins are selected when there are no coins.
	/// </summary>
	[Fact]
	public void SelectNothingFromEmptySetOfCoins()
	{
		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();

		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator);
		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: [],
			CreateUtxoSelectionParameters(),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.Empty(coins);
	}

	/// <summary>
	/// This test is to make sure no coins are selected when all coins are private.
	/// </summary>
	[Fact]
	public void SelectNothingFromFullyPrivateSetOfCoins()
	{
		const int AnonymitySet = Constants.AnonymityScoreTarget;
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coinsToSelectFrom = Enumerable
			.Range(0, 10)
			.Select(i => BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1))
			.ToList();

		// Make sure the distance from external keys is sufficient.
		foreach (var sc in coinsToSelectFrom)
		{
			var sci = BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1);
			sci.Transaction.TryAddWalletInput(BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1));
			sc.Transaction.TryAddWalletInput(sci);
		}

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();
		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator);

		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinsToSelectFrom,
			CreateUtxoSelectionParameters(),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.Empty(coins);
	}

	/// <summary>
	/// This test is to make sure private coins are selected to fund a pending payment when there is
	/// nothing left to mix. <see cref="CoinJoinCoinSelector.SelectCoinsForRound"/> selects private funds
	/// directly while retaining the fixed anonymity score target.
	/// </summary>
	[Fact]
	public void SelectPrivateCoinsToPayRegardlessOfAnonScore()
	{
		const int AnonymitySet = Constants.AnonymityScoreTarget;
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coinsToSelectFrom = Enumerable
			.Range(0, 10)
			.Select(i => BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1))
			.ToList();

		// Make sure the distance from external keys is sufficient.
		foreach (var sc in coinsToSelectFrom)
		{
			var sci = BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1);
			sci.Transaction.TryAddWalletInput(BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1));
			sc.Transaction.TryAddWalletInput(sci);
		}

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();
		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator, arePaymentsPending: () => true);

		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinsToSelectFrom,
			CreateUtxoSelectionParameters(),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.NotEmpty(coins);
		Assert.All(coins, coin => Assert.Contains(coin, coinsToSelectFrom));
	}

	/// <summary>
	/// Issue #15015: banned coins are subtracted from the candidates by the coinjoin manager, so a wallet
	/// whose only non-private coins are banned offers nothing but private coins here. A pending payment
	/// must still be funded instead of aborting the round with NoCoinsEligibleToMix.
	/// </summary>
	[Fact]
	public void SelectPrivateCoinsToPayWhenTheOnlyNonPrivateCoinsAreBanned()
	{
		const int AnonymitySet = Constants.AnonymityScoreTarget;
		var km = KeyManager.CreateNew(out _, "", Network.Main);

		// The semi-private coin is banned, hence it is not among the candidates - only private coins are.
		var coinCandidates = Enumerable
			.Range(0, 10)
			.Select(i => BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1))
			.ToList();

		// Make sure the distance from external keys is sufficient.
		foreach (var sc in coinCandidates)
		{
			var sci = BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1);
			sci.Transaction.TryAddWalletInput(BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1));
			sc.Transaction.TryAddWalletInput(sci);
		}

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();
		var coinJoinCoinSelector = new CoinJoinCoinSelector(
			generator,
			arePaymentsPending: () => true);

		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinCandidates,
			CreateUtxoSelectionParameters(),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.NotEmpty(coins);
		Assert.All(coins, coin => Assert.Contains(coin, coinCandidates));
	}

	[Fact]
	public void SelectNothingFromTooSmallCoin()
	{
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coinsToSelectFrom = new[] { BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(0.00017423m), anonymitySet: 1) };
		var roundParams = WabiSabiFactory.CreateRoundParameters(new()
		{
			MinRegistrableAmount = Money.Coins(0.0001m),
			MaxRegistrableAmount = Money.Coins(430),
		});

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();

		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator);
		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinsToSelectFrom,
			UtxoSelectionParameters.FromRoundParameters(roundParams, [ScriptType.P2WPKH, ScriptType.Taproot]),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.Empty(coins);
	}

	/// <summary>
	/// This test is to make sure no coins are selected when there too small coins.
	/// </summary>
	[Fact]
	public void SelectNothingFromTooSmallSetOfCoins()
	{
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coinsToSelectFrom = new[]
		{
			BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(0.00008711m + 0.00006900m), anonymitySet: 1),
			BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(0.00008710m + 0.00006900m), anonymitySet: 1)
		};
		var roundParams = WabiSabiFactory.CreateRoundParameters(new()
		{
			MinRegistrableAmount = Money.Coins(0.0001m),
			MaxRegistrableAmount = Money.Coins(430),
		});

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();

		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator);
		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinsToSelectFrom,
			UtxoSelectionParameters.FromRoundParameters(roundParams, [ScriptType.P2WPKH, ScriptType.Taproot]),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.Empty(coins);
	}

	/// <summary>
	/// This test is to make sure the coins are selected when the selection's effective sum is exactly the smallest reasonable effective denom.
	/// </summary>
	[Fact]
	public void SelectSomethingFromJustEnoughSetOfCoins()
	{
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coinsToSelectFrom = new[]
		{
			BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(0.00008711m + 0.00006900m), anonymitySet: 1),
			BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(0.00008711m + 0.00006900m), anonymitySet: 1)
		};
		var roundParams = WabiSabiFactory.CreateRoundParameters(new()
		{
			MinRegistrableAmount = Money.Coins(0.0001m),
			MaxRegistrableAmount = Money.Coins(430),
		});

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();

		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator);
		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinsToSelectFrom,
			UtxoSelectionParameters.FromRoundParameters(roundParams, [ScriptType.P2WPKH, ScriptType.Taproot]),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.NotEmpty(coins);
	}

	/// <summary>
	/// This test is to make sure that we select the non-private coin in the set.
	/// </summary>
	[Fact]
	public void SelectNonPrivateCoinFromOneNonPrivateCoinInBigSetOfCoinsBatches()
	{
		const int AnonymitySet = Constants.AnonymityScoreTarget;
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		SmartCoin smallerAnonCoin = BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(1m), anonymitySet: AnonymitySet - 1);
		var coinsToSelectFrom = Enumerable
			.Range(0, 10)
			.Select(_ => BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(1m), anonymitySet: AnonymitySet + 1))
			.Prepend(smallerAnonCoin)
			.ToList();

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();

		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator);
		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinsToSelectFrom,
			CreateUtxoSelectionParameters(),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.Contains(smallerAnonCoin, coins);
		Assert.Equal(10, coins.Count);
	}

	/// <summary>
	/// This test is to make sure that we select the only non-private coin when it is the only coin in the wallet.
	/// </summary>
	[Fact]
	public void SelectNonPrivateCoinFromOneCoinSetOfCoins()
	{
		const int AnonymitySet = Constants.AnonymityScoreTarget;
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coinsToSelectFrom = Enumerable
			.Empty<SmartCoin>()
			.Prepend(BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(1m), anonymitySet: AnonymitySet - 1))
			.ToList();

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();

		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator);
		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinsToSelectFrom,
			CreateUtxoSelectionParameters(),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.Single(coins);
	}

	/// <summary>
	/// This test is to make sure that we select more non-private coins when they are coming from different txs.
	/// </summary>
	/// <remarks>Note randomization can make this test fail even though that's unlikely.</remarks>
	[Fact]
	public void SelectMoreNonPrivateCoinFromTwoCoinsSetOfCoins()
	{
		const int AnonymitySet = Constants.AnonymityScoreTarget;
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coinsToSelectFrom = Enumerable
			.Empty<SmartCoin>()
			.Prepend(BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(1m), anonymitySet: AnonymitySet - 1))
			.Prepend(BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(1m), anonymitySet: AnonymitySet - 1))
			.ToList();

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator(sameTxAllowance: 0);

		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator);
		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinsToSelectFrom,
			CreateUtxoSelectionParameters(),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.Equal(2, coins.Count);
	}

	/// <summary>
	/// This test is to make sure that we select more than one non-private coin.
	/// </summary>
	[Fact]
	public void SelectTwoNonPrivateCoinsFromTwoCoinsSetOfCoinsBatches()
	{
		const int AnonymitySet = Constants.AnonymityScoreTarget;
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coinsToSelectFrom = Enumerable
			.Empty<SmartCoin>()
			.Prepend(BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(1m), anonymitySet: AnonymitySet - 1))
			.Prepend(BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km), Money.Coins(1m), anonymitySet: AnonymitySet - 1))
			.ToList();

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();

		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator);
		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinsToSelectFrom,
			CreateUtxoSelectionParameters(),
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.Equal(2, coins.Count);
	}

	/// <summary>
	/// This test is to make sure no coins are selected when all coins are private.
	/// </summary>
	[Fact]
	public void SelectNothingFromFullyPrivateAndBelowMinAllowedSetOfCoins()
	{
		const int AnonymitySet = Constants.AnonymityScoreTarget;
		var km = KeyManager.CreateNew(out _, "", Network.Main);
		var coinsToSelectFrom = Enumerable
			.Range(0, 10)
			.Select(i => BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1))
			.ToList();

		var utxoSelectionParameter = CreateUtxoSelectionParameters();
		coinsToSelectFrom.Add(BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), utxoSelectionParameter.AllowedInputAmounts.Min - Money.Satoshis(1), anonymitySet: AnonymitySet - 1));

		// Make sure the distance from external keys is sufficient.
		foreach (var sc in coinsToSelectFrom)
		{
			var sci = BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1);
			sci.Transaction.TryAddWalletInput(BitcoinFactory.CreateSmartCoin(BitcoinFactory.CreateHdPubKey(km, isInternal: true), Money.Coins(1m), anonymitySet: AnonymitySet + 1));
			sc.Transaction.TryAddWalletInput(sci);
		}

		CoinJoinCoinSelectorRandomnessGenerator generator = CreateSelectorGenerator();
		var coinJoinCoinSelector = new CoinJoinCoinSelector(generator);

		var coins = coinJoinCoinSelector.SelectCoinsForRound(
			coins: coinsToSelectFrom,
			utxoSelectionParameter,
			liquidityClue: Constants.MaximumNumberOfBitcoinsMoney);

		Assert.Empty(coins);
	}

	private static CoinJoinCoinSelectorRandomnessGenerator CreateSelectorGenerator(int? sameTxAllowance = null)
	{
		GetSameTxAllowanceSelector? fixedSameTxAllowance = sameTxAllowance is not null
				? (percent) => sameTxAllowance.Value
				: null;

		var generator = new CoinJoinCoinSelectorRandomnessGenerator(
			RandomnessProviders.Insecure,
			fixedSameTxAllowance);

		return generator;
	}

	private static RoundParameters CreateMultipartyTransactionParameters()
	{
		var roundParams = WabiSabiFactory.CreateRoundParameters(new()
		{
			MinRegistrableAmount = Money.Coins(0.0001m),
			MaxRegistrableAmount = Money.Coins(430)
		});
		return roundParams;
	}

	private static UtxoSelectionParameters CreateUtxoSelectionParameters() =>
		UtxoSelectionParameters.FromRoundParameters(
			CreateMultipartyTransactionParameters(),
			[ScriptType.P2WPKH, ScriptType.Taproot]);
}
