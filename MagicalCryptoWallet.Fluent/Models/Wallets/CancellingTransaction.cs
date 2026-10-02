using MagicalCryptoWallet.Blockchain.TransactionBuilding;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public record CancellingTransaction(
	RegularTransactionModel TargetTransaction,
	BuildTransactionResult CancelTransaction,
	Amount Fee);
