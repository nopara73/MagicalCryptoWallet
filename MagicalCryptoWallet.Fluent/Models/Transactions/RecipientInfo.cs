using NBitcoin;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;

namespace MagicalCryptoWallet.Fluent.Models.Transactions;

public record RecipientInfo(
	Destination Destination,
	Money Amount,
	LabelsArray Label,
	bool IsSubtractFee = false);
