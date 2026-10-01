using Avalonia.Data.Converters;
using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.Converters;

public static class ScriptTypeConverters
{
	public static readonly IValueConverter IsSegwit = new FuncValueConverter<ScriptType, bool>(x => x == ScriptType.SegWit);
	public static readonly IValueConverter IsTaproot = new FuncValueConverter<ScriptType, bool>(x => x == ScriptType.Taproot);
	public static readonly IValueConverter ToName = new FuncValueConverter<ScriptType, string>(x => x is null ? "" : x.Name);
}
