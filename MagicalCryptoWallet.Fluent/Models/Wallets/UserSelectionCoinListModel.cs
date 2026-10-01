using System.Linq;
using System.Reactive.Linq;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public partial class UserSelectionCoinListModel(Wallet wallet, IWalletModel walletModel, SmartCoin[] selectedCoins, IServices services)
	: CoinListModel(wallet, walletModel, services)
{
	protected override CoinModel[] CreateCoinModels()
	{
		return selectedCoins.Select(CreateCoinModel).ToArray();
	}

	protected override Pocket[] GetPockets()
	{
		return
			new CoinsView(selectedCoins).GetPockets(WalletModel.Settings.AnonScoreTarget)
										.Select(x => new Pocket(x))
										.ToArray();
	}
}
