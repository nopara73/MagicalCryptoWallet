using NBitcoin;
using System.Linq;
using System.Threading.Tasks;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Models;

namespace MagicalCryptoWallet.Fluent.Models;

public partial class TransactionBroadcasterModel
{
	private readonly IServices _services;
	private readonly Network _network;

	public TransactionBroadcasterModel(IServices services, Network network)
	{
		_services = services;
		_network = network;
	}

	public SmartTransaction? Parse(string text)
	{
		if (PSBT.TryParse(text, _network, out var signedPsbt))
		{
			if (!signedPsbt.IsAllFinalized())
			{
				signedPsbt.Finalize();
			}

			return signedPsbt.ExtractSmartTransaction();
		}
		else
		{
			return new SmartTransaction(Transaction.Parse(text, _network), Height.Unknown);
		}
	}

	public Task<SmartTransaction> LoadFromFileAsync(string filePath)
	{
		return TransactionHelpers.ParseTransactionAsync(filePath, _network);
	}

	public TransactionBroadcastInfo GetBroadcastInfo(SmartTransaction transaction)
	{
		var tx = transaction.Transaction;
		var transactionId = tx.GetHash().ToString();

		var spendingSum =
			tx.Inputs
			.Select(x => x.PrevOut)
			.Select(GetOutput)
			.Aggregate<TxOut?, Money?>(Money.Zero, (acc, txout) => (acc, txout) switch
			{
				({ } a, { } t) => a + t.Value,
				_ => null
			});

		var totalAmount = new Amount(tx.Outputs.Select(x => x.Value).Sum());
		var networkFee = spendingSum is null
			? null
			: new Amount(spendingSum - totalAmount.Btc);

		return new TransactionBroadcastInfo(transactionId, totalAmount, networkFee);

		TxOut? GetOutput(OutPoint outpoint) =>
			_services.TryGetTransaction(outpoint.Hash, out var prevTxn)
				? prevTxn.Transaction.Outputs[outpoint.N]
				: null;
	}

	public Task SendAsync(SmartTransaction transaction)
	{
		return _services.SendTransactionAsync(transaction);
	}
}
