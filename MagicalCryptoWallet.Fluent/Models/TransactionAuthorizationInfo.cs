using NBitcoin;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.Transactions;

namespace MagicalCryptoWallet.Fluent.Models;

public class TransactionAuthorizationInfo
{
	public TransactionAuthorizationInfo(BuildTransactionResult buildTransactionResult)
	{
		Psbt = buildTransactionResult.Psbt;
		Transaction = buildTransactionResult.Transaction;
	}

	public SmartTransaction Transaction { get; set; }

	public PSBT Psbt { get; }
}
