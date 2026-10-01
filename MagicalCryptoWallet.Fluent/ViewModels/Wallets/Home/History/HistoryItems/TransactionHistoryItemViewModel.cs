using System.Reactive.Linq;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.History.HistoryItems;

public partial class TransactionHistoryItemViewModel : HistoryItemViewModelBase
{
	private IWalletModel _wallet;

	public TransactionHistoryItemViewModel(UiContext uiContext, IWalletModel wallet, RegularTransactionModel transaction) : base(uiContext, transaction)
	{
		_wallet = wallet;

		Transaction = transaction;
		CanBeSpedUp = transaction.CanSpeedUpTransaction && !IsChild;
		CanBeCancelled = transaction.CanCancelTransaction;
		ShowDetailsCommand = ReactiveCommand.Create(() => UiContext.Navigate().To().TransactionDetails(wallet, transaction));
		SpeedUpTransactionCommand = ReactiveCommand.CreateFromTask(async () => await OnSpeedUpTransactionAsync(transaction, CancellationToken.None), Observable.Return(CanBeSpedUp));
		CancelTransactionCommand = ReactiveCommand.Create(() => OnCancelTransaction(transaction), Observable.Return(CanBeCancelled));
		HasBeenSpedUp = transaction.HasBeenSpedUp;
	}

	public override RegularTransactionModel Transaction { get; }

	public bool TransactionOperationsVisible => CanBeCancelled || CanBeSpedUp;

	private async Task OnSpeedUpTransactionAsync(RegularTransactionModel transaction, CancellationToken cancellationToken)
	{
		try
		{
			var speedupTransaction = await _wallet.Transactions.CreateSpeedUpTransactionAsync(transaction, cancellationToken);
			UiContext.Navigate().To().SpeedUpTransactionDialog(_wallet, speedupTransaction);
		}
		catch (Exception ex)
		{
			Logger.LogError(ex);
			UiContext.Navigate().To().ShowErrorDialog(ex.ToUserFriendlyString(), "Speed Up failed", "Magical Crypto Wallet could not initiate the transaction speed up process.");
		}
	}

	private void OnCancelTransaction(RegularTransactionModel transaction)
	{
		try
		{
			var cancellingTransaction = _wallet.Transactions.CreateCancellingTransaction(transaction);
			UiContext.Navigate().To().CancelTransactionDialog(_wallet, cancellingTransaction);
		}
		catch (Exception ex)
		{
			Logger.LogError(ex);
			UiContext.Navigate().To().ShowErrorDialog(ex.ToUserFriendlyString(), "Cancel failed", "Magical Crypto Wallet could not initiate the cancelling process.");
		}
	}
}
