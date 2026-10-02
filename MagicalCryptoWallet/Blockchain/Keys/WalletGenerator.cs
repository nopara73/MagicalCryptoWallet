using NBitcoin;
using System.IO;
using System.Linq;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Userfacing;
using MagicalCryptoWallet.Wallets.Slip39;

namespace MagicalCryptoWallet.Blockchain.Keys;

public class WalletGenerator
{
	public const byte DefaultShamirShares = 3;
	public const byte DefaultShamirThreshold = 2;
	public const int MinShamirShares = 1;
	public const int MaxShamirShares = 16;
	public const int MinShamirThreshold = 1;
	public const int MaxShamirThreshold = 16;

	public WalletGenerator(string walletsDir, Network network)
	{
		WalletsDir = walletsDir;
		Network = network;
	}

	public string WalletsDir { get; private set; }
	public Network Network { get; private set; }
	public uint TipHeight { get; set; }

	public (KeyManager KeyManager, Mnemonic Mnemonic) GenerateDraft(string password, Mnemonic? mnemonic = null)
	{
		string walletFilePath = Path.Combine(WalletsDir, "Wallet.json");

		// Here we are not letting anything that will be autocorrected later. We need to generate the wallet exactly with the entered password because of compatibility.
		PasswordHelper.Guard(password);

		var km = mnemonic is null
			? KeyManager.CreateNew(out mnemonic, password, Network)
			: KeyManager.CreateNew(mnemonic, password, Network);
		km.SetFilePath(walletFilePath);
		km.SetBestHeight(TipHeight, toFile: false);
		return (km, mnemonic);
	}

	public (KeyManager KeyManager, Share[] Shares) GenerateDraft(string password, Share[]? shares = null)
	{
		string walletFilePath = Path.Combine(WalletsDir, "Wallet.json");

		// Here we are not letting anything that will be autocorrected later. We need to generate the wallet exactly with the entered password because of compatibility.
		PasswordHelper.Guard(password);

		shares ??= Shamir.Generate(
			DefaultShamirThreshold,
			DefaultShamirShares,
			GenerateShamirEntropy()).Take(DefaultShamirThreshold).ToArray();

		var km = KeyManager.CreateNew(shares, password, Network);

		km.SetBestHeight(TipHeight, toFile: false);
		km.SetFilePath(walletFilePath);
		return (km, shares);
	}

	public static byte[] GenerateShamirEntropy()
	{
		return RandomUtils.GetBytes(128 / 8);
	}
}
