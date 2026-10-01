using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Navigation;

public interface IWalletNavigation
{
	IWalletViewModel? To(IWalletModel wallet);
}

public interface IWalletSelector : IWalletNavigation
{
	IWalletViewModel? SelectedWallet { get; }

	IWalletModel? SelectedWalletModel { get; }
}
