using System.Collections.Generic;
using NBitcoin;
using ReactiveUI;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Validation;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;

namespace MagicalCryptoWallet.Fluent.ViewModels.Settings;

[AppLifetime]
[NavigationMetaData(
	Title = "Bitcoin",
	Caption = "Manage Bitcoin settings",
	Order = 1,
	Category = "Settings",
	Keywords = ["Settings", "Bitcoin", "Network", "Main", "TestNet", "TestNet4", "Signet", "RegTest", "Dust", "Attack", "Limit"],
	IconName = "settings_bitcoin_regular")]
public partial class BitcoinTabSettingsViewModel : RoutableViewModel
{
	[AutoNotify] private string _dustThreshold;

	public BitcoinTabSettingsViewModel(UiContext uiContext, ApplicationSettings settings) : base(uiContext)
	{
		Settings = settings;
		_dustThreshold = settings.DustThreshold;
		this.ValidateProperty(x => x.DustThreshold, ValidateDustThreshold);
		this.WhenAnyValue(x => x.Settings.DustThreshold)
			.Subscribe(x => DustThreshold = x);
	}

	public bool IsReadOnly => Settings.IsOverridden;
	public ApplicationSettings Settings { get; }
	public IEnumerable<Network> Networks { get; } = [Network.Main, Network.TestNet, Bitcoin.Instance.Signet, Network.RegTest];

	private void ValidateDustThreshold(IValidationErrors errors)
	{
		var dustThreshold = DustThreshold;
		if (!string.IsNullOrWhiteSpace(dustThreshold))
		{
			bool error = false;

			if (!string.IsNullOrEmpty(dustThreshold) && dustThreshold.Contains(
				',',
				StringComparison.InvariantCultureIgnoreCase))
			{
				error = true;
				errors.Add(ErrorSeverity.Error, "Use decimal point instead of comma.");
			}

			if (!decimal.TryParse(dustThreshold, out var dust) || dust < 0)
			{
				error = true;
				errors.Add(ErrorSeverity.Error, "Invalid dust attack limit.");
			}

			if (!error)
			{
				Settings.DustThreshold = dustThreshold;
			}
		}
	}
}
