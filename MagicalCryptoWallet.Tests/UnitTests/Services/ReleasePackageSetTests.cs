using System.IO;
using System.Linq;
using System.Security.Cryptography;
using MagicalCryptoWallet.ReleaseTools;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Services;

public class ReleasePackageSetTests : IDisposable
{
	private const string Version = "2.3.4";
	private readonly string _directory = Path.Combine(Path.GetTempPath(), "MagicalCryptoWallet-release-inventory-" + Guid.NewGuid());

	public ReleasePackageSetTests() => Directory.CreateDirectory(_directory);

	[Theory]
	[InlineData("2.3.4")]
	[InlineData("2.3.4.5")]
	public void AcceptsEveryTargetForOneVersion(string version)
	{
		string[] suffixes = [".msi", "-win-x64.zip", ".deb", ".AppImage", "-linux-x64.zip", "-linux-x64.tar.gz",
			"-arm64.deb", "-arm64.AppImage", "-linux-arm64.zip", "-linux-arm64.tar.gz", ".dmg", "-macOS-x64.zip", "-arm64.dmg", "-macOS-arm64.zip"];
		var names = suffixes.Select(suffix => Package(version, suffix)).Order(StringComparer.Ordinal).ToArray();
		Assert.Equal(names, ReleasePackageSet.GetPackages(_directory, version));
	}

	[Fact]
	public void RejectsMixedVersionsAndVersionPrefixCollisions()
	{
		Package();
		var old = Package("2.3.3", ".deb");
		Assert.Throws<InvalidOperationException>(() => ReleasePackageSet.GetPackages(_directory, Version));
		File.Delete(Path.Combine(_directory, old));
		Package("2.3.4.5", ".deb");
		Assert.Throws<InvalidOperationException>(() => ReleasePackageSet.GetPackages(_directory, Version));
	}

	[Fact]
	public void RejectsUnsupportedTargets()
	{
		Package(Version, "-unsupported.zip");
		Assert.Throws<InvalidOperationException>(() => ReleasePackageSet.GetPackages(_directory, Version));
	}

	[Fact]
	public void RejectsEmptyInventoryAndInvalidVersion()
	{
		Assert.Throws<InvalidOperationException>(() => ReleasePackageSet.GetPackages(_directory, Version));
		Package();
		Assert.Throws<InvalidOperationException>(() => ReleasePackageSet.GetPackages(_directory, "2.3"));
	}

	[Fact]
	public void AnnouncesExactlyCurrentPackagesAndTheirSignatures()
	{
		var package = Package();
		WriteManifest(package);
		File.WriteAllText(Path.Combine(_directory, package + ".asc"), "Synthetic package signature");
		File.WriteAllText(Path.Combine(_directory, "MagicalCryptoWallet-2.3.3.msi.asc"), "Orphaned signature");
		File.WriteAllText(Path.Combine(_directory, "notes.txt"), "Unrelated file");
		File.WriteAllText(Path.Combine(_directory, "release-announcement.json"), "Previous announcement");
		string[] expected = [package, package + ".asc", "SHA256SUMS", "SHA256SUMS.asc", "SHA256SUMS.magicalcryptowalletsig"];
		Assert.Equal(expected.Order(StringComparer.Ordinal), ReleasePackageSet.GetAnnouncementAssets(_directory, Version));
	}

	[Fact]
	public void RejectsChangedPackageAfterSigning()
	{
		var package = Package();
		WriteManifest(package);
		File.AppendAllText(Path.Combine(_directory, package), "Changed payload");
		Assert.Throws<InvalidOperationException>(() => ReleasePackageSet.GetAnnouncementAssets(_directory, Version));
	}

	[Fact]
	public void RejectsMissingPackageInManifest()
	{
		var package = Package();
		WriteManifest(package);
		Package(Version, ".deb");
		Assert.Throws<InvalidOperationException>(() => ReleasePackageSet.GetAnnouncementAssets(_directory, Version));
	}

	[Fact]
	public void RejectsPlainManifestDifferingFromSignedContents()
	{
		WriteManifest(Package());
		var signed = Path.Combine(_directory, "SHA256SUMS.asc");
		File.WriteAllText(signed, File.ReadAllText(signed).Replace("2.3.4", "2.3.3", StringComparison.Ordinal));
		Assert.Throws<InvalidOperationException>(() => ReleasePackageSet.GetAnnouncementAssets(_directory, Version));
	}

	[Fact]
	public void RejectsMalformedSignedContentsAndMissingManifests()
	{
		WriteManifest(Package());
		File.WriteAllText(Path.Combine(_directory, "SHA256SUMS.asc"), "Not signed contents");
		Assert.Throws<InvalidOperationException>(() => ReleasePackageSet.GetAnnouncementAssets(_directory, Version));
		File.Delete(Path.Combine(_directory, "SHA256SUMS.magicalcryptowalletsig"));
		Assert.Throws<InvalidOperationException>(() => ReleasePackageSet.GetAnnouncementAssets(_directory, Version));
	}

	private string Package(string version = Version, string suffix = ".msi")
	{
		var name = $"MagicalCryptoWallet-{version}{suffix}";
		File.WriteAllText(Path.Combine(_directory, name), "Synthetic package, no executable or wallet");
		return name;
	}

	private void WriteManifest(params string[] packages)
	{
		var plain = string.Concat(packages.Order(StringComparer.Ordinal).Select(name =>
			$"{Convert.ToHexStringLower(SHA256.HashData(File.ReadAllBytes(Path.Combine(_directory, name))))}  ./{name}\n"));
		File.WriteAllText(Path.Combine(_directory, "SHA256SUMS"), plain);
		File.WriteAllText(Path.Combine(_directory, "SHA256SUMS.asc"), $"-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\n{plain}-----BEGIN PGP SIGNATURE-----\nSynthetic armor; signature verification is separate\n");
		File.WriteAllText(Path.Combine(_directory, "SHA256SUMS.magicalcryptowalletsig"), "Synthetic signature; verification is separate");
	}

	public void Dispose() => Directory.Delete(_directory, recursive: true);
}
