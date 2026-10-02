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
public partial class WalletNotificationsViewModel : ViewModelBase
{
	private readonly IWalletNavigation _walletNavigation;
	[AutoNotify] private bool _isBusy;

	public WalletNotificationsViewModel(UiContext uiContext, IWalletNavigation walletNavigation) : base(uiContext)
	{
		_walletNavigation = walletNavigation;
	}

	public void StartListening()
	{
		UiContext.WalletRepository.WhenAnyValue(x => x.Wallet)
			.WhereNotNull()
			.Select(wallet => wallet.WhenAnyValue(x => x.IsLoggedIn)
				.CombineLatest(wallet.Loaded, (loggedIn, loaded) => loggedIn && loaded)
				.Select(ready => wallet.Transactions.NewTransactionArrived.Where(_ => ready))
				.Switch())
			.Switch()
			.Where(x => !UiContext.ApplicationSettings.PrivacyMode)
			.Where(x => x.EventArgs.IsNews)
			.DoAsync(x => OnNotificationReceivedAsync(x.Wallet, x.EventArgs))
			.Subscribe();
	}

	private async Task OnNotificationReceivedAsync(IWalletModel wallet, ProcessedResult e)
	{
		if (!e.IsOwnCoinJoin)
		{
			void OnClick()
			{
				if (UiContext.Navigate().IsAnyPageBusy)
				{
					return;
				}

				var wvm = _walletNavigation.To(wallet);
				wvm?.SelectTransaction(e.Transaction.GetHash());
			}

			NotificationHelpers.Show(wallet, e, OnClick);
		}

		if (_walletNavigation.WalletModel == wallet && (e.NewlyReceivedCoins.Count != 0 || e.NewlyConfirmedReceivedCoins.Count != 0))
		{
			await Task.Delay(200);
			_walletNavigation.Wallet?.SelectTransaction(e.Transaction.GetHash());
		}
	}
}
