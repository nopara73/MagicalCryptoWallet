using ReactiveUI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Wallets;
using System.Reactive.Disposables;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Fluent.ViewModels.AddWallet;

[NavigationMetaData(Title = "Success")]
public partial class AddedWalletPageViewModel : RoutableViewModel
{
	private readonly WalletSetupDraft _draft;
	private IWalletModel? _wallet;

	public AddedWalletPageViewModel(UiContext uiContext, WalletSetupDraft walletDraft, WalletCreationOptions options) : base(uiContext)
	{
		_draft = walletDraft;


		SetupCancel(enableCancel: false, enableCancelOnEscape: false, enableCancelOnPressed: false);
		EnableBack = false;

		NextCommand = ReactiveCommand.CreateFromTask(() => OnNextAsync(options));
	}



	private async Task OnNextAsync(WalletCreationOptions options)
	{
		try { _wallet ??= UiContext.WalletSetupService.Commit(_draft); }
		catch (Exception ex)
		{
			await ShowErrorAsync("Wallet setup", ex.Message, "Unable to save the wallet");
			Navigate().Clear();
			if (UiContext.Services.WalletSession.Snapshot.State == WalletSessionState.Faulted)
			{ UiContext.Navigate().To(new MagicalCryptoWallet.Fluent.ViewModels.Wallets.WalletRecoveryViewModel(UiContext), NavigationTarget.HomeScreen, NavigationMode.Clear); }
			return;
		}

		IsBusy = true;


		IsBusy = false;

		await Task.Delay(UiConstants.CloseSuccessDialogMillisecondsDelay);

		Navigate().Clear();

		UiContext.Navigate().OpenWalletHome();
	}

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		base.OnNavigatedTo(isInHistory, disposables);


		if (NextCommand is not null && NextCommand.CanExecute(default))
		{
			NextCommand.Execute(default);
		}
	}

}
