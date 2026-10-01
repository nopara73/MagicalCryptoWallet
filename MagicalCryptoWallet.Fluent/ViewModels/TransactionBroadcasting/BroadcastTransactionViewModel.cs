using System.Threading.Tasks;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.Fluent.ViewModels.TransactionBroadcasting;

[NavigationMetaData(Title = "Broadcast Transaction")]
public partial class BroadcastTransactionViewModel : RoutableViewModel
{
	public BroadcastTransactionViewModel(UiContext uiContext, SmartTransaction transaction) : base(uiContext)
	{
		SetupCancel(enableCancel: true, enableCancelOnEscape: true, enableCancelOnPressed: true);

		EnableBack = false;

		NextCommand = ReactiveCommand.CreateFromTask(async () => await OnNextAsync(transaction));

		EnableAutoBusyOn(NextCommand);

		BroadcastInfo = UiContext.TransactionBroadcaster.GetBroadcastInfo(transaction);
	}

	public TransactionBroadcastInfo BroadcastInfo { get; }

	private async Task OnNextAsync(SmartTransaction transaction)
	{
		try
		{
			await UiContext.TransactionBroadcaster.SendAsync(transaction);
			Navigate().To().Success();
		}
		catch (Exception ex)
		{
			Logger.LogError(ex);
			await ShowErrorAsync("Broadcast Transaction", ex.ToUserFriendlyString(), "It was not possible to broadcast the transaction.");
		}
	}
}
