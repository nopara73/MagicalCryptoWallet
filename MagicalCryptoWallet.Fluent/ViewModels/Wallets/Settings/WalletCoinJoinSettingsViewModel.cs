using System.Reactive.Disposables.Fluent;
using System.Reactive.Disposables;
using System.Reactive.Linq;
using NBitcoin;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.Validation;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Settings;

[NavigationMetaData(
	Title = "Coinjoin Settings",
	Caption = "Display wallet coinjoin settings",
	IconName = "nav_wallet_24_regular",
	Order = 1,
	Category = "Wallet",
	Keywords = new[] { "Wallet", "Settings", },
	NavBarPosition = NavBarPosition.None,
	NavigationTarget = NavigationTarget.DialogScreen,
	Searchable = false)]
public partial class WalletCoinJoinSettingsViewModel : RoutableViewModel, IDisposable
{
	private readonly CompositeDisposable _lifetime = new();
	private readonly IWalletModel _wallet;
	[AutoNotify] private string _plebStopThreshold;

	public WalletCoinJoinSettingsViewModel(UiContext uiContext, IWalletModel walletModel) : base(uiContext)
	{
		_wallet = walletModel;
		_plebStopThreshold = _wallet.Settings.PlebStopThreshold.ToString();
		SetupCancel(enableCancel: false, enableCancelOnEscape: true, enableCancelOnPressed: true);
		NextCommand = CancelCommand;
		this.ValidateProperty(x => x.PlebStopThreshold, ValidateThreshold);
		this.WhenAnyValue(x => x.PlebStopThreshold)
			.Skip(1)
			.Throttle(TimeSpan.FromMilliseconds(1000))
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(value =>
			{
				if (Money.TryParse(value, out var threshold) && threshold >= Money.Zero && threshold != _wallet.Settings.PlebStopThreshold)
				{
					_wallet.Settings.PlebStopThreshold = threshold;
					_wallet.Settings.Save();
				}
			}).DisposeWith(_lifetime);
	}

	private void ValidateThreshold(IValidationErrors errors)
	{
		if (!Money.TryParse(PlebStopThreshold, out var threshold) || threshold < Money.Zero)
		{
			errors.Add(ErrorSeverity.Error, "Must be a non-negative BTC amount.");
		}
	}

	public void Dispose() => _lifetime.Dispose();
}
