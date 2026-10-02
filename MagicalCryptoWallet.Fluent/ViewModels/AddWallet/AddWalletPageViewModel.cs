using System.IO;
using System.Reactive;
using System.Reactive.Disposables;
using System.Threading.Tasks;
using System.Windows.Input;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.Base;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.Fluent.ViewModels.AddWallet;

[NavigationMetaData(
	Title = "Set Up Wallet",
	Caption = "Create, connect, import or recover",
	Order = 2,
	Category = "General",
	Keywords = new[]
		{ "Wallet", "Add", "Create", "New", "Recover", "Import", "Connect", "Hardware", "ColdCard", "Trezor", "Ledger" },
	IconName = "nav_add_circle_24_regular",
	IconNameFocused = "nav_add_circle_24_filled",
	NavigationTarget = NavigationTarget.DialogScreen,
	NavBarPosition = NavBarPosition.None,
	NavBarSelectionMode = NavBarSelectionMode.Button,
	Searchable = false)]
public partial class AddWalletPageViewModel : DialogViewModelBase<Unit>
{
	public AddWalletPageViewModel(UiContext uiContext) : base(uiContext)
	{
		CreateWalletCommand = ReactiveCommand.Create(OnCreateWallet);

		ConnectHardwareWalletCommand = ReactiveCommand.Create(OnConnectHardwareWallet);

		ImportWalletCommand = ReactiveCommand.CreateFromTask(OnImportWalletAsync);

		RecoverWalletCommand = ReactiveCommand.Create(OnRecoverWallet);
	}

	public ICommand CreateWalletCommand { get; }

	public ICommand ConnectHardwareWalletCommand { get; }

	public ICommand ImportWalletCommand { get; }

	public ICommand RecoverWalletCommand { get; }

	private void OnCreateWallet()
	{
		var options = new WalletCreationOptions.AddNewWallet().WithNewWalletBackups();
		Navigate().To().WalletNamePage(options);
	}

	private void OnConnectHardwareWallet()
	{
		Navigate().To().WalletNamePage(new WalletCreationOptions.ConnectToHardwareWallet());
	}

	private async Task OnImportWalletAsync()
	{
		try
		{
			var file = await FileDialogHelper.OpenFileAsync("Import wallet file", ["json"]);

			if (file is null)
			{
				return;
			}

			var filePath = file.Path.LocalPath;
			var walletName = Path.GetFileNameWithoutExtension(filePath);

			var options = new WalletCreationOptions.ImportWallet(walletName, filePath);

			var validationError = UiContext.WalletRepository.ValidateWalletName(walletName);
			if (validationError is { })
			{
				Navigate().To().WalletNamePage(options);
				return;
			}

			var walletSettings = await UiContext.WalletRepository.NewWalletAsync(options);

			Navigate().To().AddedWalletPage(walletSettings, options);
		}
		catch (Exception ex)
		{
			Logger.LogError(ex);
			await ShowErrorAsync("Import wallet", ex.ToUserFriendlyString(), "Magical Crypto Wallet was unable to import your wallet.");
		}
	}

	private void OnRecoverWallet()
	{
		Navigate().To().WalletNamePage(new WalletCreationOptions.RecoverWallet());
	}

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		base.OnNavigatedTo(isInHistory, disposables);

		UiContext.Services.WalletManager.EnsureCanAddWallet();
		SetupCancel(enableCancel: false, enableCancelOnEscape: false, enableCancelOnPressed: false);
	}

	public async Task Activate()
	{
		var mainViewModel = UiContext.MainViewModel
			?? throw new InvalidOperationException("MainViewModel is not initialized.");

		mainViewModel.IsOobeBackgroundVisible = true;
		await NavigateDialogAsync(this, NavigationTarget.DialogScreen);
		mainViewModel.IsOobeBackgroundVisible = false;
	}
}
