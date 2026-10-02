using NBitcoin;
using System.Collections.Generic;
using MagicalCryptoWallet.FeeRateEstimation;
using MagicalCryptoWallet.Fluent.Helpers;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Helpers;

public class TransactionFeeHelperTests
{
	[Fact]
	public void MissingEstimatesAreUnavailable()
	{
		Assert.False(TransactionFeeHelper.TryGetFeeEstimates(null, Network.Main, out var estimates));
		Assert.Null(estimates);
	}

	[Fact]
	public void EmptyEstimatesAreUnavailable()
	{
		Assert.False(TransactionFeeHelper.TryGetFeeEstimates(FeeRateEstimations.Empty, Network.Main, out var estimates));
		Assert.Null(estimates);
	}

	[Fact]
	public void FilteredOutEstimatesAreUnavailable()
	{
		var feeEstimates = new FeeRateEstimations(new Dictionary<int, FeeRate>
		{
			[0] = new(1m),
			[1009] = new(1m)
		});

		Assert.False(TransactionFeeHelper.TryGetFeeEstimates(feeEstimates, Network.Main, out var estimates));
		Assert.Null(estimates);
	}

	[Fact]
	public void AvailableEstimatesArePreserved()
	{
		var feeEstimates = new FeeRateEstimations(new Dictionary<int, FeeRate> { [2] = new(1m) });

		Assert.True(TransactionFeeHelper.TryGetFeeEstimates(feeEstimates, Network.Main, out var estimates));
		Assert.Same(feeEstimates, estimates);
	}

	[Theory]
	[InlineData(false)]
	[InlineData(true)]
	public void TestNetRetainsFallbackEstimates(bool hasEmptyEstimates)
	{
		var feeEstimates = hasEmptyEstimates ? FeeRateEstimations.Empty : null;

		Assert.True(TransactionFeeHelper.TryGetFeeEstimates(feeEstimates, Network.TestNet, out var estimates));
		Assert.NotNull(estimates);
		Assert.NotEmpty(estimates.WildEstimations);
	}

	[Fact]
	public void AutomaticFeeUsesHighestEstimateRegardlessOfInputOrder()
	{
		var estimates = new FeeRateEstimations(new Dictionary<int, FeeRate>
		{
			[144] = new(1m),
			[6] = new(9m),
			[2] = new(35.125m),
			[3] = new(20m)
		});

		Assert.True(TransactionFeeHelper.TryGetHighestFeeRate(estimates, Network.Main, out var feeRate));
		Assert.Equal(new FeeRate(35.125m), feeRate);
	}

	[Fact]
	public void AutomaticFeeUsesHighestAvailableSparseEstimate()
	{
		var estimates = new FeeRateEstimations(new Dictionary<int, FeeRate>
		{
			[18] = new(7m),
			[144] = new(1m)
		});

		Assert.True(TransactionFeeHelper.TryGetHighestFeeRate(estimates, Network.RegTest, out var feeRate));
		Assert.Equal(new FeeRate(7m), feeRate);
	}

	[Fact]
	public void AutomaticFeeSupportsSingleEstimate()
	{
		var estimates = new FeeRateEstimations(new Dictionary<int, FeeRate> { [2] = new(1m) });

		Assert.True(TransactionFeeHelper.TryGetHighestFeeRate(estimates, Network.Main, out var feeRate));
		Assert.Equal(new FeeRate(1m), feeRate);
	}

	[Theory]
	[InlineData(false)]
	[InlineData(true)]
	public void AutomaticFeeRequiresUsableEstimates(bool hasEmptyEstimates)
	{
		var estimates = hasEmptyEstimates ? FeeRateEstimations.Empty : null;

		Assert.False(TransactionFeeHelper.TryGetHighestFeeRate(estimates, Network.Main, out var feeRate));
		Assert.Null(feeRate);
	}

	[Fact]
	public void AutomaticFeeRejectsFilteredOutEstimates()
	{
		var estimates = new FeeRateEstimations(new Dictionary<int, FeeRate>
		{
			[0] = new(100m),
			[1009] = new(1m)
		});

		Assert.False(TransactionFeeHelper.TryGetHighestFeeRate(estimates, Network.Main, out var feeRate));
		Assert.Null(feeRate);
	}

	[Fact]
	public void AutomaticFeeUsesHighestTestNetFallbackEstimate()
	{
		Assert.True(TransactionFeeHelper.TryGetHighestFeeRate(null, Network.TestNet, out var feeRate));
		Assert.Equal(new FeeRate(12m), feeRate);
	}
}
