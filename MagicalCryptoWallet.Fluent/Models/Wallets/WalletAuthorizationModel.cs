using System.Security;
using System.Threading.Tasks;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public sealed class WalletAuthorizationModel(WalletSession session, Wallet wallet)
{
	public async Task<WalletAuthorization?> TryAuthorizeAsync(string password)
	{
		WalletAuthorization? scope = null;
		try
		{
			scope = await Task.Run(() => WalletAuthorization.Create(wallet.KeyManager, password));
			session.CompleteOperationAuthorization(scope);
			return scope;
		}
		catch (SecurityException) { scope?.Dispose(); return null; }
		catch { scope?.Dispose(); throw; }
	}
}
