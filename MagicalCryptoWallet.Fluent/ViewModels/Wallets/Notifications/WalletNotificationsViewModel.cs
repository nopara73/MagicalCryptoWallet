using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using System.Threading.Tasks;
using MagicalCryptoWallet.Blockchain.TransactionProcessing;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Notifications;

[AppLifetime]
public partial class WalletNotificationsViewModel : ViewModelBase, IDisposable
{
	private readonly CompositeDisposable _lifetime = new();
	private readonly IWalletNavigation _walletNavigation;
	[AutoNotify] private bool _isBusy;

	public WalletNotificationsViewModel(UiContext uiContext, IWalletNavigation walletNavigation) : base(uiContext)
	{
		_walletNavigation = walletNavigation;
	}

	public void StartListening()
	{
		UiContext.WalletSetupService.WhenAnyValue(x => x.Wallet)
			.WhereNotNull()
			.Select(wallet => wallet.Transactions.NewTransactionArrived)
			.Switch()
			.Where(x => !UiContext.ApplicationSettings.PrivacyMode)
			.Where(x => x.EventArgs.IsNews)
			.Do(x => OnNotificationReceived(x.Wallet, x.EventArgs))
			.Subscribe().DisposeWith(_lifetime);
	}

	private void OnNotificationReceived(IWalletModel wallet, ProcessedResult e)
	{
		if (!e.IsOwnCoinJoin)
		{
			void OnClick()
			{
				if (UiContext.Navigate().IsAnyPageBusy)
				{
					return;
				}

				var wvm = _walletNavigation.OpenWalletHome();
				wvm?.SelectTransaction(e.Transaction.GetHash());
			}

			NotificationHelpers.Show(wallet, e, OnClick);
		}

	}
	public void Dispose() => _lifetime.Dispose();

}
