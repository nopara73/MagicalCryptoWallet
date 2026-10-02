using System.Linq;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public partial class WalletCoinsModel(Wallet wallet, IWalletModel walletModel, IServices services)
	: CoinListModel(wallet, walletModel, services)
{
	protected override Pocket[] GetPockets()
	{
		return Wallet.GetPockets().ToArray();
	}

	protected override CoinModel[] CreateCoinModels()
	{
		return Wallet.Coins.Select(CreateCoinModel).ToArray();
	}
}
