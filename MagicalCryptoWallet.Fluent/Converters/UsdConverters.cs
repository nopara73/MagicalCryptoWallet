using Avalonia.Data.Converters;
using MagicalCryptoWallet.Fluent.Extensions;

namespace MagicalCryptoWallet.Fluent.Converters;

public static class UsdConverters
{
	public static readonly IValueConverter ToUsdBtcExchangeRate =
		new FuncValueConverter<decimal, string>(n => n == 0 ? "N/A" : n.ToUsdFormatted());
}
