using System.Threading.Tasks;
using MagicalCryptoWallet.Fluent.Models;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.Authorization;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class AuthorizationHelpers
{
	public static async Task<WalletAuthorization?> AuthorizeAsync(UiContext uiContext, IWalletModel wallet, string continueText = "Continue")
	{
		if (wallet.IsWatchOnlyWallet) { return null; }
		if (await wallet.Auth.TryAuthorizeAsync("") is { } empty) { return empty; }
		return await uiContext.Navigate().To().PasswordAuthDialog(wallet, continueText).GetResultAsync();
	}

	public static async Task<bool> AuthorizeTransactionAsync(UiContext uiContext, IWalletModel wallet, TransactionAuthorizationInfo transaction)
	{
		uiContext.Services.WalletSession.EnsureReady();
		if (wallet is IHardwareWalletModel hardware)
		{
			var dialog = new HardwareWalletAuthDialogViewModel(uiContext, hardware, transaction);
			return (await uiContext.Navigate().NavigateDialogAsync(dialog)).Result;
		}
		using var authorization = await AuthorizeAsync(uiContext, wallet, "Send");
		if (authorization is null) { return false; }
		transaction.Authorization = authorization.Retain();
		transaction.Transaction = await Task.Run(() => authorization.Sign(transaction.Preview).Transaction);
		return true;
	}
}
