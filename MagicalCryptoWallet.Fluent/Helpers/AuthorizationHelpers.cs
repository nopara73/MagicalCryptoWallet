using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Fluent.Models;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.Authorization;

namespace MagicalCryptoWallet.Fluent.Helpers;

// TODO: Remove this entire class after SendViewModel is decoupled.
public static class AuthorizationHelpers
{
	public static AuthorizationDialogBase GetAuthorizationDialog(UiContext uiContext, IWalletModel wallet, BuildTransactionResult transaction)
	{
		var transactionAuthorizationInfo = new TransactionAuthorizationInfo(transaction);
		return GetAuthorizationDialog(uiContext, wallet, transactionAuthorizationInfo);
	}

	public static AuthorizationDialogBase GetAuthorizationDialog(UiContext uiContext, IWalletModel wallet, TransactionAuthorizationInfo transactionAuthorizationInfo)
	{
		if (wallet is IHardwareWalletModel hwm)
		{
			return new HardwareWalletAuthDialogViewModel(uiContext, hwm, transactionAuthorizationInfo);
		}
		else
		{
			return new PasswordAuthDialogViewModel(uiContext, wallet, "Send");
		}
	}
}
