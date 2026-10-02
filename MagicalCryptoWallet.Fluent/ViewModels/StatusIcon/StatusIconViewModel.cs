using System.Reactive.Linq;
using System.Windows.Input;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.StatusIcon;

[AppLifetime]
public partial class StatusIconViewModel : ViewModelBase
{
	[AutoNotify] private string? _versionText;

	public StatusIconViewModel(UiContext uiContext) : base(uiContext)
	{
		HealthMonitor = uiContext.HealthMonitor;

		ManualUpdateCommand = ReactiveCommand.CreateFromTask(() => UiContext.OpenBrowserAsync("https://github.com/nopara73/MagicalCryptoWallet/releases"));
		UpdateCommand = ReactiveCommand.Create(
			() =>
			{
				UiContext.ApplicationSettings.DoUpdateOnClose = true;
				AppLifetimeHelper.Shutdown();
			});

		AskMeLaterCommand = ReactiveCommand.Create(() => HealthMonitor.CheckForUpdates = false);

		OpenTorStatusSiteCommand = ReactiveCommand.CreateFromTask(() => UiContext.OpenBrowserAsync("https://status.torproject.org"));

		this.WhenAnyValue(
				x => x.HealthMonitor.UpdateAvailable,
				x => x.HealthMonitor.IsReadyToInstall,
				x => x.HealthMonitor.ClientVersion,
				(updateAvailable, isReadyToInstall, clientVersion) =>
					(updateAvailable || isReadyToInstall) && clientVersion != null)
			.Select(_ => GetVersionText())
			.BindTo(this, x => x.VersionText);
	}

	public HealthMonitor HealthMonitor { get; }

	public ICommand OpenTorStatusSiteCommand { get; }

	public ICommand UpdateCommand { get; }

	public ICommand ManualUpdateCommand { get; }

	public ICommand AskMeLaterCommand { get; }


	public bool IsTorDisabled => HealthMonitor.TorStatus == MagicalCryptoWallet.Models.TorStatus.TurnedOff;

	private string GetVersionText()
	{
		if (HealthMonitor.IsReadyToInstall)
		{
			return $"Version {HealthMonitor.ClientVersion} is now ready to install";
		}
		else if (HealthMonitor.UpdateAvailable)
		{
			return $"Version {HealthMonitor.ClientVersion} is now available";
		}

		return string.Empty;
	}
}
