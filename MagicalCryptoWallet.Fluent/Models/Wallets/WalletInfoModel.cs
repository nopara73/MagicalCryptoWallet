using NBitcoin.WalletPolicies;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Userfacing;
using MagicalCryptoWallet.Wallets;
using static MagicalCryptoWallet.Blockchain.Keys.WpkhWalletPolicyHelper;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public partial class WalletInfoModel
{
	public WalletInfoModel(Wallet wallet)
	{
		var network = wallet.Network;
		if (!wallet.KeyManager.IsWatchOnly)
		{
			var secret = PasswordHelper.GetMasterExtKey(wallet.KeyManager, wallet.Password, out _);

			ExtendedMasterPrivateKey = secret.GetWif(network).ToWif();
			ExtendedAccountPrivateKey = secret.Derive(wallet.KeyManager.SegwitAccountKeyPath).GetWif(network).ToWif();
			ExtendedMasterZprv = secret.ToZPrv(network);

			// TODO: Should work for every type of wallet, temporarily disabling it.
			WpkhWalletPolicy = wallet.KeyManager.GetWpkhWalletPolicy(wallet.Password, network);
		}

		SegWitExtendedAccountPublicKey = wallet.KeyManager.SegwitExtPubKey.ToString(network);
		TaprootExtendedAccountPublicKey = wallet.KeyManager.TaprootExtPubKey?.ToString(network);

		SegWitAccountKeyPath = $"m/{wallet.KeyManager.SegwitAccountKeyPath}";
		TaprootAccountKeyPath = $"m/{wallet.KeyManager.TaprootAccountKeyPath}";
		MasterKeyFingerprint = wallet.KeyManager.MasterFingerprint?.ToString();
	}

	public string SegWitExtendedAccountPublicKey { get; }

	public string? TaprootExtendedAccountPublicKey { get; }

	public string SegWitAccountKeyPath { get; }

	public string TaprootAccountKeyPath { get; }

	public string? MasterKeyFingerprint { get; }

	public string? ExtendedMasterPrivateKey { get; }

	public string? ExtendedAccountPrivateKey { get; }

	public string? ExtendedMasterZprv { get; }

	public WalletPolicy? WpkhWalletPolicy { get; }
}
