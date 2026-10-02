using System.Globalization;
using NBitcoin;
using MagicalCryptoWallet.Fluent.Converters;
using MagicalCryptoWallet.Fluent.Extensions;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Extensions;

public class HistoryDateFormattingTests
{
	[Theory]
	[InlineData(2026, 1, 1, 0, 0, 0, "2026-01-01 00:00")]
	[InlineData(2026, 12, 31, 23, 59, 59, "2026-12-31 23:59")]
	[InlineData(2024, 2, 29, 12, 0, 59, "2024-02-29 12:00")]
	[InlineData(2026, 3, 1, 0, 1, 0, "2026-03-01 00:01")]
	[InlineData(2026, 4, 7, 15, 45, 0, "2026-04-07 15:45")]
	[InlineData(1, 1, 1, 0, 0, 0, "0001-01-01 00:00")]
	[InlineData(9999, 12, 31, 23, 59, 59, "9999-12-31 23:59")]
	public void TimestampsUsePaddedGregorianDatesAndTwentyFourHourTime(int year, int month, int day, int hour, int minute, int second, string expected)
	{
		var value = new DateTime(year, month, day, hour, minute, second);

		Assert.Equal(expected, value.ToUserFacingString());
		Assert.Equal(expected[..10], value.ToUserFacingString(withTime: false));
	}

	[Theory]
	[InlineData(DateTimeKind.Local)]
	[InlineData(DateTimeKind.Utc)]
	[InlineData(DateTimeKind.Unspecified)]
	public void DateTimeFormattingPreservesTheSuppliedClockTime(DateTimeKind kind)
	{
		var value = new DateTime(2026, 1, 1, 0, 15, 0, kind);

		Assert.Equal("2026-01-01 00:15", value.ToUserFacingString());
	}

	[Theory]
	[InlineData(-14)]
	[InlineData(0)]
	[InlineData(14)]
	public void DateTimeOffsetFormattingPreservesTheExistingLocalTimeConversion(int offsetHours)
	{
		var value = new DateTimeOffset(2026, 1, 1, 0, 15, 59, TimeSpan.FromHours(offsetHours));
		var local = value.LocalDateTime;

		Assert.Equal(local.ToString("yyyy-MM-dd HH:mm", CultureInfo.InvariantCulture), value.ToUserFacingString());
		Assert.Equal(local.ToString("yyyy-MM-dd", CultureInfo.InvariantCulture), value.ToUserFacingString(withTime: false));
		Assert.Equal(value.ToUserFacingString(), value.ToLocalTime().ToUserFacingString());
	}

	[Fact]
	public void RecentDatesUseTheSameAbsoluteTimestampFormat()
	{
		var today = DateTime.Today;
		foreach (var value in new[] { today.AddMinutes(1), today.AddDays(-1).AddHours(23), today.AddDays(1) })
		{
			Assert.Equal(value.ToString("yyyy-MM-dd HH:mm", CultureInfo.InvariantCulture), value.ToUserFacingString());
		}
	}

	[Theory]
	[InlineData(123_456L)]
	[InlineData(-123_456L)]
	[InlineData(0L)]
	public void ExistingUnsignedAmountPartsRetainTheirMagnitudeAndPresentation(long satoshis)
	{
		var amount = Money.Satoshis(satoshis);
		var magnitude = Money.Satoshis(Math.Abs(satoshis));
		var rendered = RenderAmountParts(amount);

		Assert.Equal(RenderAmountParts(magnitude), rendered);
		Assert.DoesNotContain("+", rendered);
		Assert.DoesNotContain("-", rendered);
	}

	private static string RenderAmountParts(Money amount)
	{
		var culture = CultureInfo.InvariantCulture;
		return $"{MoneyConverters.ToBtcIrrelevantOnly.Convert(amount, typeof(string), null, culture)}{MoneyConverters.ToBtcRelevantOnly.Convert(amount, typeof(string), null, culture)}";
	}

	[Fact]
	public void RegionalDateAndTimeSettingsDoNotChangeThePresentation()
	{
		// This also runs when the test process uses invariant globalization.
		var regional = (CultureInfo)CultureInfo.InvariantCulture.Clone();
		regional.DateTimeFormat.TimeSeparator = ".";
		regional.DateTimeFormat.DateSeparator = "/";
		regional.DateTimeFormat.ShortDatePattern = "dd/MM/yyyy";
		regional.DateTimeFormat.ShortTimePattern = "h.mm tt";
		var originalCulture = CultureInfo.CurrentCulture;
		var originalUiCulture = CultureInfo.CurrentUICulture;
		try
		{
			CultureInfo.CurrentCulture = regional;
			CultureInfo.CurrentUICulture = regional;
			var value = new DateTime(2026, 4, 7, 15, 45, 0);

			Assert.Equal("2026-04-07 15:45", value.ToUserFacingString());
			Assert.Equal("2026-04-07", value.ToUserFacingString(withTime: false));
		}
		finally
		{
			CultureInfo.CurrentCulture = originalCulture;
			CultureInfo.CurrentUICulture = originalUiCulture;
		}
	}
}
