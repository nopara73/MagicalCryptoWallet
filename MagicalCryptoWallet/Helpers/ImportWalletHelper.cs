using System.Threading.Tasks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Helpers;

public static class ImportWalletHelper
{
	public static Task<KeyManager> ImportWalletAsync(WalletSession walletSession, string filePath)
	{
		walletSession.EnsureCanConfigure();
		return Task.Run(() =>
		{
			var keys = KeyManager.FromFile(filePath);
			keys.SetBestHeight(0, toFile: false);
			keys.SetFilePath(walletSession.WalletDirectories.NewWalletFilePath);
			return keys;
		});
	}
}
