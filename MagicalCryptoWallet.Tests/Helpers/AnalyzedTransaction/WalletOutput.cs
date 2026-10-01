using NBitcoin;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Models;

namespace MagicalCryptoWallet.Tests.Helpers.AnalyzedTransaction;

public record WalletOutput(SmartCoin Coin)
{
	public double Anonymity => Coin.AnonymitySet;

	public SmartCoin ToSmartCoin() => Coin;

	public ForeignOutput ToForeignOutput()
	{
		return new ForeignOutput(Coin.Transaction.Transaction, Coin.Index);
	}

	public static WalletOutput Create(Money amount, HdPubKey hdPubKey)
	{
		ForeignOutput output = ForeignOutput.Create(amount, hdPubKey.P2wpkhScript);
		SmartTransaction smartTransaction = new(output.Transaction, Height.Unknown);
		SmartCoin smartCoin = new(smartTransaction, output.Index, hdPubKey);
		smartTransaction.TryAddWalletOutput(smartCoin);
		return new WalletOutput(smartCoin);
	}
}
