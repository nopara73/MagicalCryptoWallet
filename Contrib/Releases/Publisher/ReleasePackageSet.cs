using System.Security.Cryptography;
using System.Text.Json;
using System.Text.RegularExpressions;

namespace MagicalCryptoWallet.ReleaseTools;

internal static class ReleasePackageSet
{
	private static readonly string[] Suffixes = ReadSuffixes();
	private static readonly string[] Extensions = Suffixes.Select(suffix => suffix[suffix.IndexOf('.')..]).Distinct(StringComparer.Ordinal).ToArray();
	private static readonly string[] Manifests = ["SHA256SUMS", "SHA256SUMS.asc", "SHA256SUMS.magicalcryptowalletsig"];

	internal static string[] GetPackages(string directory, string version)
	{
		if (!Regex.IsMatch(version, @"\A[0-9]+\.[0-9]+\.[0-9]+(?:\.[0-9]+)?\z", RegexOptions.CultureInvariant)
			|| !Version.TryParse(version, out var parsed) || parsed.ToString() != version)
		{
			throw new InvalidOperationException("Expected a numeric 3- or 4-part release version.");
		}
		var expected = Suffixes.Select(suffix => $"MagicalCryptoWallet-{version}{suffix}").ToHashSet(StringComparer.Ordinal);
		var packages = Directory.EnumerateFiles(directory).Select(path => Path.GetFileName(path)!)
			.Where(name => name.StartsWith("MagicalCryptoWallet-", StringComparison.Ordinal)
				&& Extensions.Any(extension => name.EndsWith(extension, StringComparison.Ordinal)))
			.Order(StringComparer.Ordinal).ToArray();
		if (packages.Length == 0 || packages.Any(package => !expected.Contains(package)))
		{
			throw new InvalidOperationException("No packages, mixed release versions or unsupported targets.");
		}
		return packages;
	}

	internal static string[] GetAnnouncementAssets(string directory, string version)
	{
		var packages = GetPackages(directory, version);
		if (Manifests.Any(name => !File.Exists(Path.Combine(directory, name))))
		{
			throw new InvalidOperationException("Sign the manifest before preparing an announcement.");
		}
		var expected = packages.Select(name =>
		{
			using var stream = File.OpenRead(Path.Combine(directory, name));
			return $"{Convert.ToHexStringLower(SHA256.HashData(stream))}  ./{name}";
		}).ToArray();
		var plain = File.ReadAllText(Path.Combine(directory, "SHA256SUMS")).Replace("\r\n", "\n").TrimEnd();
		if (plain != string.Join("\n", expected))
		{
			throw new InvalidOperationException("The manifest must contain exactly the selected release's package hashes.");
		}
		var armored = File.ReadAllText(Path.Combine(directory, "SHA256SUMS.asc")).Replace("\r\n", "\n");
		var start = armored.IndexOf("\n\n", StringComparison.Ordinal);
		var end = armored.IndexOf("-----BEGIN PGP SIGNATURE-----", StringComparison.Ordinal);
		if (!armored.StartsWith("-----BEGIN PGP SIGNED MESSAGE-----", StringComparison.Ordinal) || start < 0 || end <= start)
		{
			throw new InvalidOperationException("Invalid signed checksum manifest.");
		}
		var content = string.Join("\n", armored[(start + 2)..end].Split('\n').Select(line => line.StartsWith("- ", StringComparison.Ordinal) ? line[2..] : line)).TrimEnd();
		if (content != plain)
		{
			throw new InvalidOperationException("The checksum manifest differs from its signed contents.");
		}
		return packages.Concat(packages.Select(name => name + ".asc").Where(name => File.Exists(Path.Combine(directory, name))))
			.Concat(Manifests).Order(StringComparer.Ordinal).ToArray();
	}

	private static string[] ReadSuffixes()
	{
		using var stream = typeof(ReleasePackageSet).Assembly.GetManifestResourceStream("MagicalCryptoWallet.PackageSuffixes.json")!;
		return JsonSerializer.Deserialize<string[]>(stream)!;
	}
}
