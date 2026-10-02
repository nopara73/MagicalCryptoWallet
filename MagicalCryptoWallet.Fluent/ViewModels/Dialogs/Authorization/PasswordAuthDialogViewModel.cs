using System.Threading.Tasks;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.Base;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Dialogs.Authorization;

[NavigationMetaData(Title = "Enter your passphrase", NavigationTarget = NavigationTarget.CompactDialogScreen)]
public partial class PasswordAuthDialogViewModel : DialogViewModelBase<WalletAuthorization?>
{
	private readonly IWalletModel _wallet;
	[AutoNotify] private string _password = "";
	[AutoNotify] private bool _hasAuthorizationFailed;
	[AutoNotify] private string _authorizationFailedMessage = "The passphrase is incorrect. Please try again.";

	public PasswordAuthDialogViewModel(UiContext uiContext, IWalletModel wallet, string continueText = "Continue") : base(uiContext)
	{
		_wallet = wallet;
		ContinueText = continueText;
		SetupCancel(enableCancel: true, enableCancelOnEscape: true, enableCancelOnPressed: true);
		NextCommand = ReactiveCommand.CreateFromTask(AuthorizeAsync);
		EnableAutoBusyOn(NextCommand);
	}
	public string ContinueText { get; }
	private async Task AuthorizeAsync()
	{
		var password = Password;
		Password = "";
		var authorization = await _wallet.Auth.TryAuthorizeAsync(password);
		HasAuthorizationFailed = authorization is null;
		if (authorization is { })
		{
			bool transferred = false;
			try
			{
				if (authorization.CompatibilityPasswordUsed)
				{
					await ShowErrorAsync(Title, MagicalCryptoWallet.Userfacing.PasswordHelper.CompatibilityPasswordWarnMessage, "Compatibility passphrase was used");
				}
				if (!IsDialogOpen) { return; }
				Close(DialogResultKind.Normal, authorization);
				transferred = true;
			}
			finally { if (!transferred) { authorization.Dispose(); } }
		}
	}
}
