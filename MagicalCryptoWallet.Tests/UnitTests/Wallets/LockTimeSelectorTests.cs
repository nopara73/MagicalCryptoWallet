using NBitcoin;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Wallets;

public class LockTimeSelectorTests
{
	[Fact]
	public void GetLockTimeBasedOnDistributionTest()
	{
		var lockTimeSelector = new LockTimeSelector(Random.Shared);

		uint tipHeight = 600_000;
		LockTime lockTime = lockTimeSelector.GetLockTimeBasedOnDistribution(tipHeight);

		if (lockTime.Value == 0)
		{
			Assert.Equal(LockTime.Zero, lockTime);
		}
		else
		{
			Assert.InRange(lockTime.Value, tipHeight - 99, tipHeight);
		}
	}

	[Theory]
	[InlineData(0u)]
	[InlineData(1u)]
	[InlineData(10u)]
	[InlineData(600_000u)]
	public void EveryDistributionBranchProducesAnImmediatelyFinalLockTime(uint tipHeight)
	{
		// Future locks fail mempool policy; subtracting 99 from a short chain must not wrap uint.
		foreach (var distributionValue in new[] { 0.0, 0.91, 0.978, 0.999 })
		{
			var selector = new LockTimeSelector(new FixedRandom(distributionValue));
			Assert.InRange(selector.GetLockTimeBasedOnDistribution(tipHeight).Value, 0u, tipHeight);
		}
	}

	private sealed class FixedRandom(double value) : Random
	{
		public override double NextDouble() => value;
		public override int Next(int minValue, int maxValue) => maxValue - 1;
	}
}
