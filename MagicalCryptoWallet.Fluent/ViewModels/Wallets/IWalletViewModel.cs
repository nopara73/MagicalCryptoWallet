using NBitcoin;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets;

public interface IWalletViewModel
{
	void SelectTransaction(uint256 txid);
}
