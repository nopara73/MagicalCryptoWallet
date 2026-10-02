using NBitcoin;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models;

public class TransactionAuthorizationInfo : IDisposable
{
	public TransactionAuthorizationInfo(BuildTransactionResult buildTransactionResult)
	{
		Preview = buildTransactionResult;
		Psbt = buildTransactionResult.Psbt;
		Transaction = buildTransactionResult.Transaction;
	}

	public SmartTransaction Transaction { get; set; }

	public PSBT Psbt { get; }
	public BuildTransactionResult Preview { get; }
	public WalletAuthorization? Authorization { get; set; }
	public void Dispose() { Authorization?.Dispose(); Authorization = null; }
}
