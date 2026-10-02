using System.Reactive;
using System.Reactive.Linq;
using ReactiveUI;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public partial class WalletPrivacyModel
{
	public WalletPrivacyModel(IWalletModel walletModel, Wallet wallet)
	{
		ProgressUpdated =
			walletModel.Transactions.TransactionProcessed
					   .StartWith(Unit.Default)
					   .ObserveOn(RxApp.MainThreadScheduler);

		Progress = ProgressUpdated.Select(_ => wallet.GetPrivacyPercentage());

		IsWalletPrivate = ProgressUpdated.Select(x => wallet.IsWalletPrivate());
	}

	public IObservable<Unit> ProgressUpdated { get; }

	public IObservable<int> Progress { get; }

	public IObservable<bool> IsWalletPrivate { get; }
}
