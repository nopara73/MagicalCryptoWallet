using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.Models;

public record TransactionBroadcastInfo(string TransactionId, Amount TotalAmount, Amount? NetworkFee);
