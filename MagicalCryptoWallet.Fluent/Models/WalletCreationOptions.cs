using NBitcoin;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Hwi.Models;
using MagicalCryptoWallet.Wallets.Slip39;

namespace MagicalCryptoWallet.Fluent.Models;

public abstract record WalletCreationOptions
{
	public record AddNewWallet(WalletBackup? SelectedWalletBackup = null, WalletBackup[]? WalletBackups = null)
		: WalletCreationOptions
	{
		public AddNewWallet WithNewWalletBackups()
		{
			var recoveryWordsBackup = new RecoveryWordsBackup(
				Password: "",
				Mnemonic: new Mnemonic(Wordlist.English, WordCount.Twelve));

			var multiShareBackupSettings = new MultiShareBackupSettings();

			var multiShareBackup = new MultiShareBackup(
				Settings: new MultiShareBackupSettings(),
				Shares: Shamir.Generate(
					multiShareBackupSettings.Threshold,
					multiShareBackupSettings.Shares,
					WalletGenerator.GenerateShamirEntropy()),
				Password: "");

			return this with
			{
				SelectedWalletBackup = recoveryWordsBackup,
				WalletBackups = [recoveryWordsBackup, multiShareBackup]
			};
		}
	}

	public record ConnectToHardwareWallet(
		HwiEnumerateEntry? Device = null) : WalletCreationOptions;

	public record ImportWallet(
		string? FilePath = null) : WalletCreationOptions;

	public record RecoverWallet(
		WalletBackup? WalletBackup = null,
		int? MinGapLimit = null,
		uint? BirthHeight = null) : WalletCreationOptions;
}
