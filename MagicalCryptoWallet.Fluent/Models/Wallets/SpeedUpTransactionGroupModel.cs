using System.Collections.Generic;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public class SpeedUpTransactionGroupModel : RegularTransactionModel
{
	public SpeedUpTransactionGroupModel(TransactionType type) : base(type)
	{
	}

	public required IReadOnlyList<TransactionModel> Children { get; init; }
}
