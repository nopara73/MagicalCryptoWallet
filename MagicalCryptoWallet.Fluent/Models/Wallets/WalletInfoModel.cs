using NBitcoin.WalletPolicies;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Userfacing;
using MagicalCryptoWallet.Wallets;
using static MagicalCryptoWallet.Blockchain.Keys.WpkhWalletPolicyHelper;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public partial class WalletInfoModel : IDisposable
{
	private readonly Wallet _wallet;
	public WalletInfoModel(Wallet wallet)
	{
		var network = wallet.Network;
		_wallet = wallet;
		if (wallet.KeyManager.MasterFingerprint is { } fingerprint)
		{
			WpkhWalletPolicy = WalletPolicy.Parse($"wpkh([{fingerprint}/{wallet.KeyManager.SegwitAccountKeyPath}]{wallet.KeyManager.SegwitExtPubKey.ToString(network)}/<0;1>/*)", network);
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

	public string? ExtendedMasterPrivateKey { get; private set; }

	public string? ExtendedAccountPrivateKey { get; private set; }

	public string? ExtendedMasterZprv { get; private set; }

	public WalletPolicy? WpkhWalletPolicy { get; }
	public void Reveal(WalletAuthorization authorization)
	{
		if (!ReferenceEquals(_wallet.KeyManager, authorization.KeyManager)) { throw new InvalidOperationException("Authorization belongs to another wallet."); }
		var secret = authorization.MasterKey;
		ExtendedMasterPrivateKey = secret.GetWif(_wallet.Network).ToWif();
		ExtendedAccountPrivateKey = secret.Derive(_wallet.KeyManager.SegwitAccountKeyPath).GetWif(_wallet.Network).ToWif();
		ExtendedMasterZprv = secret.ToZPrv(_wallet.Network);
	}
	public void Dispose() { ExtendedMasterPrivateKey = ExtendedAccountPrivateKey = ExtendedMasterZprv = null; }
}
