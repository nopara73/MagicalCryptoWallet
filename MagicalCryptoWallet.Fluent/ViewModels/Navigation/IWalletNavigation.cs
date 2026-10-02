using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Navigation;

public interface IWalletNavigation
{
	IWalletViewModel? OpenWalletHome();
}
