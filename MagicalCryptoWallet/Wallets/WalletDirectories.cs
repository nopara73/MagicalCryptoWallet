using NBitcoin;
using System.Collections.Generic;
using System.IO;
using System.Linq;

namespace MagicalCryptoWallet.Wallets;

public class WalletDirectories
{
	private readonly string _workDir;
	public const string WalletsDirName = "Wallets";
	public const string WalletFileExtension = "json";

	public WalletDirectories(Network network, string workDir)
	{
		_workDir = workDir;
		Network = network;
		WalletsDir = network == Network.Main
			? Path.Combine(workDir, WalletsDirName)
			: Path.Combine(workDir, WalletsDirName, network.ToString());

		Directory.CreateDirectory(WalletsDir);
	}

	public string WalletsDir { get; }

	public Network Network { get; }

	public string ConfiguredWalletFilePath => Path.Combine(WalletsDir, ".wallet");

	public string? GetConfiguredWalletName()
	{
		if (File.Exists(ConfiguredWalletFilePath))
		{
			var name = File.ReadAllText(ConfiguredWalletFilePath);
			ValidateConfiguredWalletName(name);
			if (!File.Exists(GetWalletFilePaths(name + ".json")))
			{
				throw new FileNotFoundException("The configured wallet file is missing. Restore it from your backup before starting the application.", GetWalletFilePaths(name + ".json"));
			}
			return name;
		}

		var names = EnumerateWalletFiles().Select(file => Path.GetFileNameWithoutExtension(file.Name)).Order(StringComparer.Ordinal).ToArray();
		var previousWalletName = names.Length > 1 ? ReadPreviousWalletName() : null;
		return names.Contains(previousWalletName, StringComparer.Ordinal) ? previousWalletName : names.FirstOrDefault();
	}

	public void SetConfiguredWalletName(string walletName)
	{
		ValidateConfiguredWalletName(walletName);
		if (File.Exists(ConfiguredWalletFilePath) && File.ReadAllText(ConfiguredWalletFilePath) == walletName)
		{
			return;
		}
		var temporaryPath = ConfiguredWalletFilePath + "." + Guid.NewGuid().ToString("N") + ".tmp";
		try
		{
			File.WriteAllText(temporaryPath, walletName);
			File.Move(temporaryPath, ConfiguredWalletFilePath, overwrite: true);
		}
		finally
		{
			File.Delete(temporaryPath);
		}
	}

	private string? ReadPreviousWalletName()
	{
		var path = Path.Combine(_workDir, "UiConfig.json");
		if (!File.Exists(path))
		{
			return null;
		}
		try
		{
			using var document = System.Text.Json.JsonDocument.Parse(File.ReadAllText(path));
			return document.RootElement.ValueKind == System.Text.Json.JsonValueKind.Object && document.RootElement.TryGetProperty("LastSelectedWallet", out var name) && name.ValueKind == System.Text.Json.JsonValueKind.String ? name.GetString() : null;
		}
		catch (Exception ex) when (ex is IOException or System.Text.Json.JsonException)
		{
			Logging.Logger.LogWarning(ex);
			return null;
		}
	}

	private static void ValidateConfiguredWalletName(string walletName)
	{
		if (!Blockchain.Keys.WalletGenerator.ValidateWalletName(walletName) || walletName.Contains('/') || walletName.Contains('\\'))
		{
			throw new InvalidDataException("The configured wallet name is invalid.");
		}
	}

	public string GetWalletFilePaths(string walletName)
	{
		if (!walletName.EndsWith($".{WalletFileExtension}", StringComparison.OrdinalIgnoreCase))
		{
			walletName = $"{walletName}.{WalletFileExtension}";
		}
		return Path.Combine(WalletsDir, walletName);
	}

	public IEnumerable<FileInfo> EnumerateWalletFiles()
	{
		var walletsDirInfo = new DirectoryInfo(WalletsDir);
		var walletsDirExists = walletsDirInfo.Exists;
		var searchPattern = $"*.{WalletFileExtension}";
		var searchOption = SearchOption.TopDirectoryOnly;
		IEnumerable<FileInfo> result;

		
		if (!walletsDirExists)
		{
			return Enumerable.Empty<FileInfo>();
		}

		result = walletsDirInfo.EnumerateFiles(searchPattern, searchOption);
		
		return result.OrderByDescending(t => t.LastAccessTimeUtc);
	}

	public string GetNextWalletName(string prefix = "Random Wallet")
	{
		int i = 1;
		var walletNames = EnumerateWalletFiles().Select(x => Path.GetFileNameWithoutExtension(x.Name));
		while (true)
		{
			var walletName = i == 1 ? prefix : $"{prefix} {i}";

			if (!walletNames.Contains(walletName))
			{
				return walletName;
			}

			i++;
		}
	}
}
