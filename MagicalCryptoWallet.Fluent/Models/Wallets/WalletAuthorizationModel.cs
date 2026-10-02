using System.Security;
using System.Threading.Tasks;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public sealed class WalletAuthorizationModel(Wallet wallet)
{
	public async Task<WalletAuthorization?> TryAuthorizeAsync(string password)
	{
		try
		{
			return await Task.Run(() => WalletAuthorization.Create(wallet.KeyManager, password));
		}
		catch (SecurityException) { return null; }
	}
}
