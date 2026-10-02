using MagicalCryptoWallet.WabiSabi.Client.CoinJoinProgressEvents;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.WabiSabi.Client.StatusChangedEvents;

public class CoinJoinStatusEventArgs : StatusChangedEventArgs
{
	public CoinJoinStatusEventArgs(Wallet wallet, CoinJoinProgressEventArgs coinJoinProgressEventArgs) : base(wallet)
	{
		CoinJoinProgressEventArgs = coinJoinProgressEventArgs;
	}

	public CoinJoinProgressEventArgs CoinJoinProgressEventArgs { get; }
}
