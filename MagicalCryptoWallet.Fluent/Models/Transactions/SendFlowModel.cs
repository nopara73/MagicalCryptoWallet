using System.Linq;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Send;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Transactions;

/// <summary>The send flow always uses the wallet's automatically selected coins.</summary>
public record SendFlowModel(Wallet Wallet)
{
	public ICoinsView AvailableCoins => Wallet.Coins;

	public TransactionInfo? TransactionInfo { get; init; }

	public decimal AvailableAmountBtc => AvailableAmount.ToDecimal(MoneyUnit.BTC);

	public Money AvailableAmount => AvailableCoins.TotalAmount();

	public Pocket[] GetPockets() =>
		AvailableCoins.GetPockets(Wallet.AnonScoreTarget).Select(x => new Pocket(x)).ToArray();
}
