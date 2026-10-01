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
	private readonly IWalletSelector _walletSelector;
	[AutoNotify] private bool _isBusy;

	public WalletNotificationsViewModel(UiContext uiContext, IWalletSelector walletSelector) : base(uiContext)
	{
		_walletSelector = walletSelector;
	}

	public void StartListening()
	{
		UiContext.WalletRepository.Wallets
			.Connect()
			.AutoRefresh(x => x.IsLoggedIn)
			.Filter(x => x.IsLoggedIn)
			.FilterOnObservable(x => x.Loaded.Select(s => s))
			.MergeMany(x => x.Transactions.NewTransactionArrived)
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

				var wvm = _walletSelector.To(wallet);
				wvm?.SelectTransaction(e.Transaction.GetHash());
			}

			NotificationHelpers.Show(wallet, e, OnClick);
		}

		if (_walletSelector.SelectedWalletModel == wallet && (e.NewlyReceivedCoins.Count != 0 || e.NewlyConfirmedReceivedCoins.Count != 0))
		{
			await Task.Delay(200);
			_walletSelector.SelectedWallet?.SelectTransaction(e.Transaction.GetHash());
		}
	}
}
