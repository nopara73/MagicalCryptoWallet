using NBitcoin;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Coinjoins;
using MagicalCryptoWallet.WabiSabi.Client;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.ViewModels;

/// <summary>
/// The cost rows the single-coinjoin and the grouped-coinjoins details screens share.
/// </summary>
public class CoinjoinCostsViewModelTests
{
	[Fact]
	public void RecordedCostsUseTheTotalFee()
	{
		var costs = new CoinjoinCosts(Money.Satoshis(700), Money.Satoshis(300), Money.Zero);
		var viewModel = new CoinjoinCostsViewModel(CreateAmount);

		viewModel.Update(costs, amount: Money.Satoshis(-1_000));

		// The total includes both the mining fee and wasted dust.
		Assert.Equal(Money.Satoshis(1_000), viewModel.TotalFeeAmount?.Btc);

		Assert.False(viewModel.ArePaymentsVisible);
		Assert.Null(viewModel.PaymentsAmount);
	}

	[Fact]
	public void PaymentsAreShownSeparatelyAndAreNotCountedAsFees()
	{
		var costs = new CoinjoinCosts(Money.Satoshis(700), Money.Satoshis(300), Money.Coins(0.5m));
		var viewModel = new CoinjoinCostsViewModel(CreateAmount);

		viewModel.Update(costs, amount: -Money.Coins(0.5m) - Money.Satoshis(1_000));

		Assert.True(viewModel.ArePaymentsVisible);
		Assert.Equal(Money.Coins(0.5m), viewModel.PaymentsAmount?.Btc);

		// The wallet sent half a bitcoin, but the coinjoin only cost it the fee.
		Assert.Equal(Money.Satoshis(1_000), viewModel.TotalFeeAmount?.Btc);
	}

	[Fact]
	public void CoinjoinsMadeBeforeTheCostsWereRecordedFallBackToASingleFigure()
	{
		var viewModel = new CoinjoinCostsViewModel(CreateAmount);

		viewModel.Update(coinjoinCosts: null, amount: Money.Satoshis(-1_234));

		Assert.False(viewModel.ArePaymentsVisible);
		Assert.Equal(Money.Satoshis(1_234), viewModel.TotalFeeAmount?.Btc);
	}

	[Fact]
	public void SelectingAnOlderCoinjoinAfterANewerOneClearsPayments()
	{
		// The same view model is reused as the transaction list updates, so nothing may linger.
		var viewModel = new CoinjoinCostsViewModel(CreateAmount);

		viewModel.Update(new CoinjoinCosts(Money.Satoshis(700), Money.Satoshis(300), Money.Coins(0.5m)), Money.Satoshis(-1_000));
		viewModel.Update(coinjoinCosts: null, amount: Money.Satoshis(-1_234));

		Assert.False(viewModel.ArePaymentsVisible);
		Assert.Null(viewModel.PaymentsAmount);
	}

	private static Amount CreateAmount(Money? money) => new(money ?? Money.Zero);
}
