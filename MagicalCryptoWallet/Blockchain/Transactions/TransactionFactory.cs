using NBitcoin;
using System.Collections.Generic;
using System.Linq;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Exceptions;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Blockchain.Transactions;

public class TransactionFactory
{
	public TransactionFactory(Network network, KeyManager keyManager, ICoinsView coins, ITransactionStore transactionStore, string password = "", WalletAuthorization? authorization = null)
	{
		Network = network;
		KeyManager = keyManager;
		Coins = coins;
		_transactionStore = transactionStore;
		_password = password;
		_authorization = authorization;
		if (authorization is not null && !ReferenceEquals(authorization.KeyManager, keyManager)) { throw new InvalidOperationException("Authorization belongs to a different wallet."); }
	}

	public Network Network { get; }
	public KeyManager KeyManager { get; }
	public ICoinsView Coins { get; }
	private readonly string _password;
	private readonly WalletAuthorization? _authorization;
	private readonly ITransactionStore _transactionStore;

	public BuildTransactionResult BuildTransaction(
		TransactionParameters parameters,
		Func<LockTime>? lockTimeSelector = null)
	{
		lockTimeSelector ??= () => LockTime.Zero;

		var payments = parameters.PaymentIntent;
		long totalAmount = payments.TotalAmount.Satoshi;
		if (totalAmount is < 0 or > Constants.MaximumNumberOfSatoshis)
		{
			throw new ArgumentOutOfRangeException($"{nameof(payments)}.{nameof(payments.TotalAmount)} sum cannot be smaller than 0 or greater than {Constants.MaximumNumberOfSatoshis}.");
		}

		// Get allowed coins to spend.
		var availableCoinsView = Coins.Unspent();
		if (parameters.AllowDoubleSpend && parameters.AllowedInputs is not null)
		{
			var doubleSpends = new List<SmartCoin>();
			foreach (var input in parameters.AllowedInputs)
			{
				if (((CoinsRegistry)Coins).AsAllCoinsView().TryGetByOutPoint(input, out var coin)
					&& coin.SpenderTransaction is not null
					&& !coin.SpenderTransaction.Confirmed)
				{
					doubleSpends.Add(coin);
				}
			}
			availableCoinsView = new CoinsView(availableCoinsView.ToList().Concat(doubleSpends));
		}

		List<SmartCoin> allowedSmartCoinInputs = parameters.AllowUnconfirmed // Inputs that can be used to build the transaction.
			? availableCoinsView.ToList()
			: availableCoinsView.Confirmed().ToList();
		if (parameters.AllowedInputs is not null) // If allowedInputs are specified then select the coins from them.
		{
			if (!parameters.AllowedInputs.Any())
			{
				throw new ArgumentException($"{nameof(parameters.AllowedInputs)} is not null, but empty.");
			}

			allowedSmartCoinInputs = allowedSmartCoinInputs
				.Where(x => parameters.AllowedInputs.Any(y => y.Hash == x.TransactionId && y.N == x.Index))
				.ToList();

			// Add those that have the same script, because common ownership is already exposed.
			// But only if the user didn't click the "max" button. In this case he'd send more money than what he'd think.
			if (payments.ChangeStrategy != ChangeStrategy.AllRemainingCustom)
			{
				var allScripts = allowedSmartCoinInputs.Select(x => x.ScriptPubKey).ToHashSet();
				foreach (var coin in availableCoinsView.Where(x => !allowedSmartCoinInputs.Any(y => x.TransactionId == y.TransactionId && x.Index == y.Index)))
				{
					if (!(parameters.AllowUnconfirmed || coin.Confirmed))
					{
						continue;
					}

					if (allScripts.Contains(coin.ScriptPubKey))
					{
						allowedSmartCoinInputs.Add(coin);
					}
				}
			}
		}

		var builder = Network.CreateTransactionBuilder();
		builder.SetVersion(2);
		builder.StandardTransactionPolicy.MinRelayTxFee = Constants.MinRelayFeeRate;
		builder.SetCoinSelector(new SmartCoinSelector(allowedSmartCoinInputs));
		builder.AddCoins(allowedSmartCoinInputs.Select(c => c.Coin));
		builder.SetLockTime(lockTimeSelector());

		foreach (var request in payments.Requests.Where(x => x.Amount is MoneyRequest.Value).Select(x => (x.Destination, Amount: (MoneyRequest.Value)x.Amount, x.Amount.SubtractFee)))
		{
			builder.Send(request.Destination.ScriptPubKey, request.Amount.Amount);
			if (request.SubtractFee)
			{
				builder.SubtractFees();
			}
		}

		HdPubKey? changeHdPubKey;

		if (payments.TryGetCustomRequest(out DestinationRequest? customChange))
		{
			var changeScript = customChange.Destination.ScriptPubKey;
			KeyManager.TryGetKeyForScriptPubKey(changeScript, out HdPubKey? hdPubKey);
			changeHdPubKey = hdPubKey;

			var changeStrategy = payments.ChangeStrategy;
			if (changeStrategy == ChangeStrategy.Custom)
			{
				builder.SetChange(customChange.Destination.ScriptPubKey);
			}
			else if (changeStrategy == ChangeStrategy.AllRemainingCustom)
			{
				builder.SendAllRemaining(customChange.Destination.ScriptPubKey);
			}
			else
			{
				throw new NotSupportedException(payments.ChangeStrategy.ToString());
			}
		}
		else
		{
			changeHdPubKey = KeyManager.GetNextChangeKey();

			builder.SetChange(changeHdPubKey.GetAssumedScriptPubKey());
		}

		builder.SendEstimatedFees(parameters.FeeRate);

		var psbt = builder.BuildPSBT(false);

		// For sub-1 sat/vB fee rates, NBitcoin's FeeRate truncates because _FeePerK is a long
		// (e.g. 0.1 sat/vB Ã— 141 vB = 14.1 -> 14 sats -> effective 0.099 sat/vB).
		// Rebuild with a FeeRate whose _FeePerK is ceiled to guarantee the fee covers the target.
		if (parameters.FeeRate.SatoshiPerByte < 1m && psbt.TryGetVirtualSize(out var estimatedVSize))
		{
			var adjustedFee = parameters.FeeRate.GetAdjustedFee(estimatedVSize);
			var truncatedFee = parameters.FeeRate.GetFee(estimatedVSize);
			if (truncatedFee < adjustedFee)
			{
				var ceilFeePerK = (long)Math.Ceiling(adjustedFee.Satoshi * 1000m / estimatedVSize);
				builder.SendEstimatedFees(new FeeRate(Money.Satoshis(ceilFeePerK)));
				psbt = builder.BuildPSBT(false);
			}
		}

		var spentCoins = psbt.Inputs.Select(txin => allowedSmartCoinInputs.First(y => y.Outpoint == txin.PrevOut)).ToArray();

		var realToSend = payments.Requests
			.Select(t =>
				(label: t.Labels,
					destination: t.Destination,
					amount: psbt.Outputs.FirstOrDefault(o => o.ScriptPubKey == t.Destination.ScriptPubKey)?.Value))
			.Where(x => x.amount is not null);

		if (!psbt.TryGetFee(out var fee))
		{
			throw new InvalidOperationException("Impossible to get the fees of the PSBT, this should never happen.");
		}

		if (!psbt.TryGetVirtualSize(out var vSize)) //builder.EstimateSize(psbt.ExtractTransaction(), true);
		{
			throw new InvalidOperationException("It was not possible to estimate the size of the transaction");
		}

		// Do some checks
		Money totalSendAmountNoFee = realToSend.Sum(x => x.amount);
		if (totalSendAmountNoFee == Money.Zero)
		{
			throw new InvalidOperationException("The amount after subtracting the fee is too small to be sent.");
		}

		Money totalOutgoingAmountNoFee;
		if (changeHdPubKey is null)
		{
			totalOutgoingAmountNoFee = totalSendAmountNoFee;
		}
		else
		{
			totalOutgoingAmountNoFee = realToSend.Where(x => !changeHdPubKey.ContainsScript(x.destination.ScriptPubKey)).Sum(x => x.amount);
		}

		decimal totalOutgoingAmountNoFeeDecimal = totalOutgoingAmountNoFee.ToDecimal(MoneyUnit.BTC);
		decimal feeDecimal = fee.ToDecimal(MoneyUnit.BTC);

		decimal feePercentage;
		if (payments.ChangeStrategy == ChangeStrategy.AllRemainingCustom)
		{
			// In this scenario since the amount changes as the fee changes, we need to compare against the total sum / 2,
			// as with this, we will make sure the fee cannot be higher than the amount.
			decimal inputSumDecimal = spentCoins.Sum(x => x.Amount.ToDecimal(MoneyUnit.BTC));
			feePercentage = 100 * (feeDecimal / (inputSumDecimal / 2));
		}
		else
		{
			// In this scenario the amount is fixed, so we can compare against it.
			// Cannot divide by zero, so use the closest number we have to zero.
			decimal totalOutgoingAmountNoFeeDecimalDivisor = totalOutgoingAmountNoFeeDecimal == 0 ? decimal.MinValue : totalOutgoingAmountNoFeeDecimal;
			feePercentage = 100 * (feeDecimal / totalOutgoingAmountNoFeeDecimalDivisor);
		}
		if (feePercentage > 100 && !parameters.OverrideFeeOverpaymentProtection)
		{
			throw new TransactionFeeOverpaymentException(feePercentage);
		}

		// Build the transaction

		psbt = MagicalCryptoWallet.Mcw.Psbt.McwPsbtMetadata.Enrich(psbt, KeyManager, _transactionStore);

		Transaction tx;
		if (!parameters.TryToSign)
		{
			tx = psbt.GetGlobalTransaction();
		}
		else
		{
			IEnumerable<Key> signingKeys = _authorization?.GetSecrets(spentCoins.Select(x => x.ScriptPubKey).ToArray()) ?? KeyManager.GetSecrets(_password, spentCoins.Select(x => x.ScriptPubKey).ToArray());
			builder = builder.AddKeys(signingKeys.ToArray());

			builder.SignPSBT(psbt);

			psbt.Finalize();
			tx = psbt.ExtractTransaction();

			var checkResults = builder.Check(tx).ToList();
			if (checkResults.Count > 0)
			{
				Logger.LogDebug($"Found policy error(s)! First error: '{checkResults[0]}'.");
				throw new InvalidTxException(tx, checkResults);
			}
		}

		var smartTransaction = new SmartTransaction(tx, labels: LabelsArray.Merge(payments.Requests.Select(x => x.Labels)));
		foreach (var coin in spentCoins)
		{
			smartTransaction.TryAddWalletInput(coin);
		}
		var label = LabelsArray.Merge(payments.Requests.Select(x => x.Labels).Concat(smartTransaction.WalletInputs.Select(x => x.HdPubKey.Labels)));

		for (var i = 0U; i < tx.Outputs.Count; i++)
		{
			TxOut output = tx.Outputs[i];
			if (KeyManager.TryGetKeyForScriptPubKey(output.ScriptPubKey, out HdPubKey? foundKey))
			{
				var smartCoin = new SmartCoin(smartTransaction, i, foundKey);
				label = LabelsArray.Merge(label, smartCoin.HdPubKey.Labels); // foundKey's label is already added to the coinLabel.
				smartTransaction.TryAddWalletOutput(smartCoin);
			}
		}

		// New labels will be added to the HdPubKey only when tx will be successfully broadcasted.
		Dictionary<HdPubKey, LabelsArray> hdPubKeysWithNewLabels = new();

		foreach (var coin in smartTransaction.WalletOutputs)
		{
			var foundPaymentRequest = payments.Requests.FirstOrDefault(x => x.Destination.ScriptPubKey == coin.ScriptPubKey);

			// If change then we concatenate all the labels.
			// The foundKeyLabel has already been added previously, so no need to concatenate.
			if (foundPaymentRequest is null) // Then it's auto-change.
			{
				hdPubKeysWithNewLabels.Add(coin.HdPubKey, label);
			}
			else
			{
				hdPubKeysWithNewLabels.Add(coin.HdPubKey, LabelsArray.Merge(coin.HdPubKey.Labels, foundPaymentRequest.Labels));
			}
		}

		var sign = parameters.TryToSign;

		Logger.LogDebug($"Built tx: {totalOutgoingAmountNoFee.ToString(fplus: false, trimExcessZero: true)} BTC. Fee: {fee.Satoshi} sats. Vsize: {vSize} vBytes. Fee/Total ratio: {feePercentage:0.#}%. Tx hash: {tx.GetHash()}.");
		return new BuildTransactionResult(smartTransaction, psbt, sign, fee, feePercentage, hdPubKeysWithNewLabels);
	}

}
