using NBitcoin;
using ReactiveUI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.WabiSabi.Client;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Coinjoins;


public partial class CoinjoinCostsViewModel : ReactiveObject
{
	private readonly Func<Money?, Amount> _createAmount;

	[AutoNotify] private Amount? _totalFeeAmount;
	[AutoNotify] private Amount? _paymentsAmount;
	[AutoNotify] private bool _arePaymentsVisible;

	public CoinjoinCostsViewModel(Func<Money?, Amount> createAmount)
	{
		_createAmount = createAmount;
	}

	public void Update(CoinjoinCosts? coinjoinCosts, Money amount)
	{
		if (coinjoinCosts is { } costs)
		{
			TotalFeeAmount = _createAmount(costs.TotalFee);

			ArePaymentsVisible = costs.PaymentsTotal != Money.Zero;
			PaymentsAmount = ArePaymentsVisible ? _createAmount(costs.PaymentsTotal) : null;
		}
		else
		{
			// allow backwards compatibility with transactions that were created before the costs were recorded
			TotalFeeAmount = _createAmount(Math.Abs(amount));

			ArePaymentsVisible = false;
			PaymentsAmount = null;
		}
	}
}
