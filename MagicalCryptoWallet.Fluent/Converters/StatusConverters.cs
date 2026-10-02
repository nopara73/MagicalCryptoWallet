using Avalonia.Data.Converters;
using MagicalCryptoWallet.Models;

namespace MagicalCryptoWallet.Fluent.Converters;

public static class StatusConverters
{
	public static readonly IValueConverter TorStatusToString =
		new FuncValueConverter<TorStatus, string>(x => x switch
		{
			TorStatus.Running => "is running",
			TorStatus.NotRunning => "is not running",
			TorStatus.TurnedOff => "is turned off",
			{ } => x.ToString()
		});

	public static readonly IValueConverter HeightToString =
		new FuncValueConverter<uint, string>(x => x == 0 ? "No data" : $"{x:N0}");


}
