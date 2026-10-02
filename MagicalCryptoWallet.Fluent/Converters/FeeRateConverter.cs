using Avalonia.Data.Converters;

namespace MagicalCryptoWallet.Fluent.Converters;

public static class FeeRateConverters
{
	public static FuncValueConverter<decimal, string> FeeRateConverter { get; } =
		new(feeRate => $"{feeRate:0.0#####} sat/vByte");
}
