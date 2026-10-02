using System.Collections.Generic;
using System.Linq;
using NBitcoin;
using ReactiveUI;
using MagicalCryptoWallet.Blockchain.TransactionBroadcasting;
using MagicalCryptoWallet.FeeRateEstimation;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Validation;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Wallets.Exchange;

namespace MagicalCryptoWallet.Fluent.ViewModels.Settings;

[AppLifetime]
[NavigationMetaData(
	Title = "Connections",
	Caption = "Manage connections settings",
	Order = 3,
	Category = "Settings",
	Keywords = new[]
	{
			"Settings", "Connections", "URI", "Exchange", "Rate", "Provider", "Fee", "Estimation", "Network", "Anonymization",
			"Tor", "Terminate", "MagicalCryptoWallet", "Shut", "Reset"
	},
	IconName = "settings_general_regular")]
public partial class ConnectionsSettingsTabViewModel : RoutableViewModel
{
	public ConnectionsSettingsTabViewModel(UiContext uiContext, ApplicationSettings settings) : base(uiContext)
	{
		Settings = settings;

		if (settings.Network == Network.Main)
		{
			ExternalBroadcastProviders = ExternalTransactionBroadcaster.Providers.Select(x => x.Name);
		}
		else
		{
			ExternalBroadcastProviders = ExternalTransactionBroadcaster.TestNet4Providers.Select(x => x.Name);
		}
	}

	public bool IsReadOnly => Settings.IsOverridden;

	public ApplicationSettings Settings { get; }

	public IEnumerable<string> ExchangeRateProviders => MagicalCryptoWallet.Wallets.Exchange.ExchangeRateProviders.Providers;
	public IEnumerable<string> FeeRateEstimationProviders => FeeRateProviders.Providers;
	public IEnumerable<string> ExternalBroadcastProviders { get; }

	public IEnumerable<TorMode> TorModes =>
		Enum.GetValues(typeof(TorMode)).Cast<TorMode>();
}
