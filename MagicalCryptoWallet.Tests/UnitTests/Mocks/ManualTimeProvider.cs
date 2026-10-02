namespace MagicalCryptoWallet.Tests.UnitTests.Mocks;

internal sealed class ManualTimeProvider : TimeProvider
{
	private DateTimeOffset _now = new(2026, 1, 1, 0, 0, 0, TimeSpan.Zero);
	public override DateTimeOffset GetUtcNow() => _now;
	public void Advance(TimeSpan duration) => _now += duration;
}
