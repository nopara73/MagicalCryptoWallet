using MagicalCryptoWallet.Crypto.Randomness;

namespace MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;

public delegate int GetSameTxAllowanceSelector(int percent);

/// <summary>Privacy-preserving input ordering and same-transaction allowance.</summary>
public class CoinJoinCoinSelectorRandomnessGenerator
{
	private readonly GetSameTxAllowanceSelector _sameTxAllowanceSelector;

	public CoinJoinCoinSelectorRandomnessGenerator(RandomnessProvider rnd, GetSameTxAllowanceSelector? fixedSameTxAllowance = null)
	{
		Rnd = rnd;
		_sameTxAllowanceSelector = fixedSameTxAllowance ?? DefaultGetRandomBiasedSameTxAllowance;
	}

	public RandomnessProvider Rnd { get; }
	public int GetRandomBiasedSameTxAllowance(int percent) => _sameTxAllowanceSelector(percent);

	private int DefaultGetRandomBiasedSameTxAllowance(int percent)
	{
		for (int num = 0; num <= 100; num++)
		{
			if (Rnd.GetInt(100) < percent)
			{
				return num;
			}
		}

		return 0;
	}
}
