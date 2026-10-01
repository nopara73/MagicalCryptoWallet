using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.Transactions;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public record SpeedupTransaction(
	SmartTransaction TargetTransaction,
	BuildTransactionResult BoostingTransaction,
	bool AreWePayingTheFee,
	Amount Fee
	);
