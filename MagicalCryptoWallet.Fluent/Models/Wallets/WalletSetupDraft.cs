using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

/// <summary>Unsaved setup output. Only WalletSetupService can commit it to the session.</summary>
public sealed class WalletSetupDraft
{
	internal WalletSetupDraft(KeyManager keys) { Keys = keys; WalletType = WalletHelpers.GetType(keys); }
	internal KeyManager Keys { get; }
	public WalletType WalletType { get; }
}
