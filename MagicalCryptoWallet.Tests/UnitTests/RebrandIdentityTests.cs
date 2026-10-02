using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.WabiSabi.Coordinator;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class RebrandIdentityTests
{
	[Fact]
	public void CoordinatorFeesRequireExplicitOperatorKey()
	{
		var fresh = new WabiSabiConfig();
		fresh.Validate();
		Assert.False(fresh.CollectCoordinatorFees);
		Assert.Null(fresh.CoordinatorExtPubKey);
		Assert.Throws<InvalidOperationException>(() => new WabiSabiConfig { CollectCoordinatorFees = true }.Validate());
		var configured = new WabiSabiConfig { CollectCoordinatorFees = true, CoordinatorExtPubKey = ExtKey.CreateFromSeed(new byte[32]).Neuter() };
		configured.Validate();
		Assert.NotEqual(configured.DeriveCoordinatorScript(1), configured.DeriveCoordinatorScript(2));
	}

	[Fact]
	public async Task FreshWalletStorageOnlyImportsExplicitlyAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		string otherWallet = Path.Combine(root, "existing-application", "synthetic.json");
		Directory.CreateDirectory(Path.GetDirectoryName(otherWallet)!);
		// Deterministic software keys are synthetic and contain no real funds.
		var source = KeyManager.CreateNew(new Mnemonic(SingleWalletTests.SyntheticMnemonic), "", Network.RegTest, otherWallet);
		source.ToFile();
		byte[] original = await File.ReadAllBytesAsync(otherWallet);
		var directories = new WalletDirectories(Network.RegTest, Path.Combine(root, "MagicalCryptoWallet", "Client"));
		var manager = new WalletSession(Network.RegTest, directories, _ => throw new InvalidOperationException("No wallet is started during this test."));
		Assert.Empty(Directory.GetFiles(directories.WalletsDir, "*.json"));
		var imported = await ImportWalletHelper.ImportWalletAsync(manager, otherWallet);
		directories.Commit(imported);
		Assert.Single(Directory.GetFiles(directories.WalletsDir, "*.json"));
		Assert.Equal(source.SegwitExtPubKey, imported.SegwitExtPubKey);
		Assert.Equal(original, await File.ReadAllBytesAsync(otherWallet));
		using var instance = new SingleInstanceChecker(directories.WalletsDir);
		Assert.True(instance.IsFirstInstance());
		Assert.Equal(".magicalcryptowallet-lock", Path.GetFileName(instance.LockFilePath));
		using var duplicate = new SingleInstanceChecker(directories.WalletsDir);
		Assert.False(duplicate.IsFirstInstance());
		using var independent = new SingleInstanceChecker(Path.GetDirectoryName(otherWallet)!);
		Assert.True(independent.IsFirstInstance());
		await manager.StopAsync(CancellationToken.None);
	}

	[Fact]
	public async Task SignedManifestMustMatchPlainChecksumsAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		string plain = Path.Combine(root, "SHA256SUMS");
		string signed = plain + ".asc";
		await File.WriteAllTextAsync(plain, "abc  synthetic-package.zip\n");
		await File.WriteAllTextAsync(signed, "-----BEGIN PGP SIGNED MESSAGE-----\nHash: SHA256\n\nabc  synthetic-package.zip\n-----BEGIN PGP SIGNATURE-----\nsynthetic\n");
		await ReleaseDownloader.VerifyManifestContentAsync(plain, signed, CancellationToken.None);
		await File.WriteAllTextAsync(plain, "tampered  synthetic-package.zip\n");
		await Assert.ThrowsAsync<InvalidOperationException>(() => ReleaseDownloader.VerifyManifestContentAsync(plain, signed, CancellationToken.None));
	}
}
