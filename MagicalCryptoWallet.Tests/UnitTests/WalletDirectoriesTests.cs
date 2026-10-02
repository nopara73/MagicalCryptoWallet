using NBitcoin;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class WalletDirectoriesTests
{
	private async Task<string> CleanupWalletDirectoriesAsync(string baseDir)
	{
		var walletsPath = Path.Combine(baseDir, WalletDirectories.WalletsDirName);
		await IoHelpers.TryDeleteDirectoryAsync(walletsPath);

		return walletsPath;
	}

	[Fact]
	public async Task CreatesWalletDirectoriesAsync()
	{
		var baseDir = Common.GetWorkDir();
		string walletsPath = await CleanupWalletDirectoriesAsync(baseDir);

		_ = new WalletDirectories(Network.Main, baseDir);
		Assert.True(Directory.Exists(walletsPath));

		// Testing what happens if the directories are already exist.
		_ = new WalletDirectories(Network.Main, baseDir);
		Assert.True(Directory.Exists(walletsPath));
	}

	[Fact]
	public async Task TestPathsAsync()
	{
		var baseDir = Common.GetWorkDir();
		await CleanupWalletDirectoriesAsync(baseDir);

		var mainWd = new WalletDirectories(Network.Main, baseDir);
		Assert.Equal(Network.Main, mainWd.Network);
		Assert.Equal(Path.Combine(baseDir, "Wallets"), mainWd.WalletsDir);

		var testWd = new WalletDirectories(Network.TestNet, baseDir);
		Assert.Equal(Network.TestNet, testWd.Network);
		Assert.Equal(Path.Combine(baseDir, "Wallets", "TestNet4"), testWd.WalletsDir);

		var regWd = new WalletDirectories(Network.RegTest, baseDir);
		Assert.Equal(Network.RegTest, regWd.Network);
		Assert.Equal(Path.Combine(baseDir, "Wallets", "RegTest"), regWd.WalletsDir);
	}

	[Fact]
	public async Task NewWalletUsesFixedFileInsideTheActiveNetworkAsync()
	{
		var root = await Common.GetEmptyWorkDirAsync();
		var main = new WalletDirectories(Network.Main, root);
		var regtest = new WalletDirectories(Network.RegTest, root);
		Assert.Equal(Path.Combine(root, "Wallets", "Wallet.json"), main.NewWalletFilePath);
		Assert.Equal(Path.Combine(root, "Wallets", "RegTest", "Wallet.json"), regtest.NewWalletFilePath);
		Assert.Null(main.ResolveConfiguredWalletFile());
		Assert.Null(regtest.ResolveConfiguredWalletFile());
	}


}
