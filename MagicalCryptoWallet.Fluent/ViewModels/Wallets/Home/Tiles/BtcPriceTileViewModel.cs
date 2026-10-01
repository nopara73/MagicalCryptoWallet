using System.Reactive.Linq;
using ReactiveUI;
using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.Tiles;

public partial class BtcPriceTileViewModel : ActivatableViewModel
{
	[AutoNotify] private decimal _usdPerBtc;

	public BtcPriceTileViewModel(UiContext uiContext, AmountProvider amountProvider) : base(uiContext)
	{
		amountProvider.BtcToUsdExchangeRate
			.ObserveOn(RxApp.MainThreadScheduler)
			.StartWith(amountProvider.UsdExchangeRate)
			.Subscribe(x => UsdPerBtc = x);
	}
}
