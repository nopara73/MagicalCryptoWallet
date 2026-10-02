using System.IO;
using System.Security.Cryptography;
using System.Text.Json;
using MagicalCryptoWallet.Blockchain.Keys;

namespace MagicalCryptoWallet.Wallets;

public sealed class WalletDirectories
{
	private readonly string _workDir;
	public const string WalletsDirName = "Wallets";
	public const string WalletFileExtension = "json";
	public WalletDirectories(Network network, string workDir)
	{
		_workDir = workDir;
		Network = network;
		WalletsDir = network == Network.Main ? Path.Combine(workDir, WalletsDirName) : Path.Combine(workDir, WalletsDirName, network.ToString());
		Directory.CreateDirectory(WalletsDir);
	}
	public Network Network { get; }
	public string WalletsDir { get; }
	public string ConfiguredWalletFilePath => Path.Combine(WalletsDir, ".wallet");
	public string NewWalletFilePath => Path.Combine(WalletsDir, "Wallet.json");
	internal string SetupJournalPath => Path.Combine(WalletsDir, ".wallet-setup");

	public string? ResolveConfiguredWalletFile()
	{
		RecoverSetup();
		return LegacyWalletDiscovery.Resolve(WalletsDir, _workDir, ConfiguredWalletFilePath);
	}
	internal void PersistConfiguredFile(string filePath)
	{
		var stem = Path.GetFileNameWithoutExtension(filePath);
		LegacyWalletDiscovery.ValidateFileStem(stem);
		if (File.Exists(ConfiguredWalletFilePath))
		{
			if (File.ReadAllText(ConfiguredWalletFilePath) == stem) { return; }
			throw new InvalidOperationException("A different wallet file is already configured.");
		}
		var temporary = ConfiguredWalletFilePath + "." + Guid.NewGuid().ToString("N") + ".tmp";
		try { File.WriteAllText(temporary, stem); File.Move(temporary, ConfiguredWalletFilePath, overwrite: false); }
		finally { File.Delete(temporary); }
	}
	public void Commit(KeyManager draft)
	{
		if (ResolveConfiguredWalletFile() is not null || File.Exists(NewWalletFilePath))
		{
			throw new InvalidOperationException("A wallet file already exists. Setup cannot overwrite it.");
		}
		var temporary = Path.Combine(WalletsDir, ".Wallet." + Guid.NewGuid().ToString("N") + ".tmp");
		bool claimed = false;
		bool journalOwned = false;
		draft.SetFilePath(temporary);
		try
		{
			draft.ToFile();
			var journal = new SetupJournal(Path.GetFileName(temporary), Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(temporary))));
			using (var stream = new FileStream(SetupJournalPath, FileMode.CreateNew, FileAccess.Write, FileShare.None))
			{
				journalOwned = true;
				JsonSerializer.Serialize(stream, journal);
				stream.Flush(flushToDisk: true);
			}
			File.Move(temporary, NewWalletFilePath, overwrite: false);
			claimed = true;
			PersistConfiguredFile(NewWalletFilePath);
			File.Delete(SetupJournalPath);
		}
		finally
		{
			draft.SetFilePath(NewWalletFilePath);
			File.Delete(temporary);
			if (!claimed && journalOwned) { File.Delete(SetupJournalPath); }
		}
	}
	private void RecoverSetup()
	{
		if (!File.Exists(SetupJournalPath)) { return; }
		var journal = JsonSerializer.Deserialize<SetupJournal>(File.ReadAllText(SetupJournalPath)) ?? throw new InvalidDataException("The wallet setup journal is invalid.");
		if (journal.TemporaryFile is not { Length: 44 } name || !name.StartsWith(".Wallet.", StringComparison.Ordinal) || !name.EndsWith(".tmp", StringComparison.Ordinal) || !Guid.TryParseExact(name.AsSpan(8, 32), "N", out _) || journal.Sha256 is not { Length: 64 } hash || !hash.All(Uri.IsHexDigit))
		{
			throw new InvalidDataException("The wallet setup journal has an invalid path.");
		}
		var temporary = Path.Combine(WalletsDir, journal.TemporaryFile);
		var candidate = File.Exists(NewWalletFilePath) ? NewWalletFilePath : temporary;
		if (!File.Exists(candidate) || Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(candidate))) != journal.Sha256)
		{
			throw new InvalidDataException("Interrupted wallet setup needs recovery. The committed file does not match its journal.");
		}
		// Reject unsupported or corrupt keys before changing any recovery evidence.
		_ = KeyManager.FromFile(candidate);
		if (candidate == temporary) { File.Move(temporary, NewWalletFilePath, overwrite: false); }
		PersistConfiguredFile(NewWalletFilePath);
		File.Delete(SetupJournalPath);
		File.Delete(temporary);
	}
	private record SetupJournal(string TemporaryFile, string Sha256);
}
