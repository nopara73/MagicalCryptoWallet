using MagicalCryptoWallet.Blockchain.Keys;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

/// <summary>Unsaved setup output. Only WalletSetupService can commit it to the session.</summary>
public sealed class WalletSetupDraft
{
	internal WalletSetupDraft(KeyManager keys) { Keys = keys; }
	internal KeyManager Keys { get; }
}
