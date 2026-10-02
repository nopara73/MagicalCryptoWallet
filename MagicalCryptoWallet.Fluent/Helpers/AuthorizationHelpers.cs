using System.Threading.Tasks;
using MagicalCryptoWallet.Fluent.Models;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class AuthorizationHelpers
{
	public static async Task<WalletAuthorization?> AuthorizeAsync(UiContext uiContext, IWalletModel wallet, string continueText = "Continue")
	{
		var authorization = await wallet.Auth.TryAuthorizeAsync("")
			?? await uiContext.Navigate().To().PasswordAuthDialog(wallet, continueText).GetResultAsync();
		if (authorization is null) { return null; }
		try
		{
			// A dialog dismissed during password derivation must not authorize background CoinJoin.
			uiContext.Services.WalletSession.CompleteOperationAuthorization(authorization);
			return authorization;
		}
		catch { authorization.Dispose(); throw; }
	}

	public static async Task<bool> AuthorizeTransactionAsync(UiContext uiContext, IWalletModel wallet, TransactionAuthorizationInfo transaction)
	{
		uiContext.Services.WalletSession.EnsureReady();
		using var authorization = await AuthorizeAsync(uiContext, wallet, "Send");
		if (authorization is null) { return false; }
		transaction.Authorization = authorization.Retain();
		transaction.Transaction = await Task.Run(() => authorization.Sign(transaction.Preview).Transaction);
		return true;
	}
}
