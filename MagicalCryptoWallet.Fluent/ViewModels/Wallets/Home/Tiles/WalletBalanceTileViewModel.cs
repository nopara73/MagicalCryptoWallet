using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.Tiles;

public class WalletBalanceTileViewModel : ActivatableViewModel
{
	public WalletBalanceTileViewModel(UiContext uiContext, IObservable<Amount> amounts) : base(uiContext)
	{
		Amounts = amounts;
	}

	public IObservable<Amount> Amounts { get; }
}
