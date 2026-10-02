using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Exceptions;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Fluent.Models.Transactions;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Send;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class TransactionHelpers
{
	public static BuildTransactionResult BuildTransaction(Wallet wallet, TransactionInfo transactionInfo, bool tryToSign = false, WalletAuthorization? authorization = null)
	{
		if (transactionInfo.IsPayToMany)
		{
			return BuildPayToManyTransaction(wallet, transactionInfo, tryToSign, authorization);
		}

		if (transactionInfo.IsOptimized)
		{
			return wallet.BuildChangelessTransaction(
				transactionInfo.Destination,
				transactionInfo.Recipient,
				transactionInfo.FeeRate,
				transactionInfo.ChangelessCoins,
				tryToSign: tryToSign, authorization: authorization);
		}

		return wallet.BuildTransaction(
			transactionInfo.Destination,
			transactionInfo.Amount,
			transactionInfo.Recipient,
			transactionInfo.FeeRate,
			transactionInfo.Coins,
			transactionInfo.SubtractFee,
			tryToSign: tryToSign, authorization: authorization);
	}

	private static BuildTransactionResult BuildPayToManyTransaction(Wallet wallet, TransactionInfo transactionInfo, bool tryToSign, WalletAuthorization? authorization)
	{
		var intent = BuildPayToManyIntent(transactionInfo);

		return wallet.BuildTransaction(
			password: string.Empty,
			payments: intent,
			feeStrategy: FeeStrategy.CreateFromFeeRate(transactionInfo.FeeRate),
			allowUnconfirmed: true,
			allowedInputs: transactionInfo.Coins.Select(c => c.Outpoint),
			tryToSign: tryToSign, authorization: authorization);
	}

	public static bool TryBuildTransactionWithoutPrevTx(
		KeyManager keyManager,
		TransactionInfo transactionInfo,
		ICoinsView allCoins,
		IEnumerable<SmartCoin> allowedCoins,
		string password,
		out Money minimumAmount)
	{
		minimumAmount = transactionInfo.IsPayToMany ? transactionInfo.TotalAmount : transactionInfo.Amount;

		try
		{
			PaymentIntent intent;
			if (transactionInfo.IsPayToMany)
			{
				intent = BuildPayToManyIntent(transactionInfo);
			}
			else
			{
				intent = new PaymentIntent(
					destination: transactionInfo.Destination,
					amount: transactionInfo.Amount,
					subtractFee: transactionInfo.SubtractFee,
					label: transactionInfo.Recipient);
			}

			var network = keyManager.GetNetwork();
			var builder = new TransactionFactory(network, keyManager, allCoins, new EmptyTransactionStore(network), password);

			TransactionParameters parameters = new (
				intent,
				transactionInfo.FeeRate,
				AllowUnconfirmed: true,
				AllowDoubleSpend: false,
				AllowedInputs: allowedCoins.Select(x => x.Outpoint),
				TryToSign: false,
				OverrideFeeOverpaymentProtection: false);

			builder.BuildTransaction(
				parameters,
				lockTimeSelector: () => LockTime.Zero);

			return true;
		}
		catch (InsufficientBalanceException ex)
		{
			minimumAmount = ex.Minimum;
		}
		catch (Exception)
		{
			// Ignore.
		}

		return false;
	}

	public static async Task<SmartTransaction> ParseTransactionAsync(string path, Network network)
	{
		var text = (await File.ReadAllTextAsync(path)).Trim();
		return new SmartTransaction(Transaction.Parse(text, network), Height.Unknown);
	}

	internal static PaymentIntent BuildPayToManyIntent(TransactionInfo transactionInfo)
	{
		var requests = transactionInfo.AllRecipients
			.Select((r, index) =>
			{
				bool subtractFee = index == 0 ? transactionInfo.SubtractFee : r.IsSubtractFee;
				return new DestinationRequest(r.Destination, MoneyRequest.Create(r.Amount, subtractFee), r.Label);
			})
			.ToArray();

		return new PaymentIntent(requests);
	}

}
