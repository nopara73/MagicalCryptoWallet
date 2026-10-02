using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.Models;

public record TransactionBroadcastInfo(string TransactionId, int InputCount, int OutputCount, Amount? InputAmount, Amount? OutputAmount, Amount? NetworkFee);
