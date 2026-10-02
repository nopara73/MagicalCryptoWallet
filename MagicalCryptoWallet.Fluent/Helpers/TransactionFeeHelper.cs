using System.Collections.Generic;
using System.Diagnostics.CodeAnalysis;
using System.Linq;
using NBitcoin;
using MagicalCryptoWallet.FeeRateEstimation;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class TransactionFeeHelper
{
	private static readonly FeeRateEstimations TestNetFeeRateEstimations = new(
		new Dictionary<int, FeeRate>
		{
			[1] = new( 17m),
			[2] = new( 12m),
			[3] = new( 9m),
			[6] = new( 9m),
			[18] = new( 2m),
			[36] = new( 2m),
			[72] = new( 2m),
			[144] = new( 2m),
			[432] = new( 1m),
			[1008] = new( 1m)
		});

	public static bool TryGetFeeEstimates(FeeRateEstimations? feeRateEstimations, Network network, [NotNullWhen(true)] out FeeRateEstimations? estimates)
	{
		if (network == Network.TestNet)
		{
			estimates = TestNetFeeRateEstimations;
			return true;
		}
		if (feeRateEstimations is { Estimations.Count: > 0 })
		{
			estimates = feeRateEstimations;
			return true;
		}

		estimates = null;
		return false;
	}

	public static bool TryGetHighestFeeRate(FeeRateEstimations? feeRateEstimations, Network network, [NotNullWhen(true)] out FeeRate? feeRate)
	{
		if (TryGetFeeEstimates(feeRateEstimations, network, out var estimates))
		{
			feeRate = estimates.Estimations.Values.MaxBy(rate => rate.SatoshiPerByte)!;
			return true;
		}

		feeRate = null;
		return false;
	}
}
