using System.IO;
using System.Text.Json;

namespace MagicalCryptoWallet.Wallets;

/// <summary>The only directory discovery path: adopt an existing installation once, without renaming any files.</summary>
internal static class LegacyWalletDiscovery
{
	internal static string? Resolve(string directory, string workDirectory, string markerPath)
	{
		if (File.Exists(markerPath))
		{
			var stem = File.ReadAllText(markerPath);
			ValidateFileStem(stem);
			var path = Path.Combine(directory, stem + ".json");
			if (!File.Exists(path)) { throw new FileNotFoundException("The configured wallet file is missing. Restore it from your backup.", path); }
			return path;
		}
		var files = Directory.EnumerateFiles(directory, "*.json", SearchOption.TopDirectoryOnly).OrderBy(Path.GetFileNameWithoutExtension, StringComparer.Ordinal).ToArray();
		if (files.Length > 1 && ReadPreviousFile(workDirectory) is { } previous)
		{
			var preferred = files.FirstOrDefault(path => Path.GetFileNameWithoutExtension(path) == previous);
			if (preferred is not null) { return preferred; }
		}
		return files.FirstOrDefault();
	}
	private static string? ReadPreviousFile(string workDirectory)
	{
		var path = Path.Combine(workDirectory, "UiConfig.json");
		if (!File.Exists(path)) { return null; }
		try
		{
			using var document = JsonDocument.Parse(File.ReadAllText(path));
			// Immutable legacy configuration field, read only during first adoption.
			return document.RootElement.ValueKind == JsonValueKind.Object && document.RootElement.TryGetProperty("LastSelectedWallet", out var value) && value.ValueKind == JsonValueKind.String ? value.GetString() : null;
		}
		catch (Exception ex) when (ex is IOException or JsonException) { Logging.Logger.LogWarning(ex); return null; }
	}
	internal static void ValidateFileStem(string stem)
	{
		var reserved = stem.Split('.')[0].ToUpperInvariant();
		if (string.IsNullOrWhiteSpace(stem) || stem is "." or ".." || stem.IndexOfAny(['/', '\\', ':', '*', '?', '"', '<', '>', '|']) >= 0 || stem.Any(char.IsControl) || stem.EndsWith('.') || stem.EndsWith(' ') || reserved is "CON" or "PRN" or "AUX" or "NUL" || (reserved.Length == 4 && (reserved.StartsWith("COM", StringComparison.Ordinal) || reserved.StartsWith("LPT", StringComparison.Ordinal)) && reserved[3] is >= '1' and <= '9'))
		{
			throw new InvalidDataException("The configured wallet file path is invalid.");
		}
	}
}
