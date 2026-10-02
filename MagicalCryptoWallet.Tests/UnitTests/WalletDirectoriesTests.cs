using NBitcoin;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Hwi.Models;
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

	[Fact]
	public void GetFriendlyNameTest()
	{
		Assert.Equal("Hardware Wallet", HardwareWalletModels.Unknown.FriendlyName());
		Assert.Equal("Coldcard", HardwareWalletModels.Coldcard.FriendlyName());
		Assert.Equal("Coldcard Simulator", HardwareWalletModels.Coldcard_Simulator.FriendlyName());
		Assert.Equal("BitBox", HardwareWalletModels.DigitalBitBox_01.FriendlyName());
		Assert.Equal("BitBox Simulator", HardwareWalletModels.DigitalBitBox_01_Simulator.FriendlyName());
		Assert.Equal("KeepKey", HardwareWalletModels.KeepKey.FriendlyName());
		Assert.Equal("KeepKey Simulator", HardwareWalletModels.KeepKey_Simulator.FriendlyName());
		Assert.Equal("Ledger Nano S", HardwareWalletModels.Ledger_Nano_S.FriendlyName());
		Assert.Equal("Ledger Nano X", HardwareWalletModels.Ledger_Nano_X.FriendlyName());
		Assert.Equal("Trezor One", HardwareWalletModels.Trezor_1.FriendlyName());
		Assert.Equal("Trezor One Simulator", HardwareWalletModels.Trezor_1_Simulator.FriendlyName());
		Assert.Equal("Trezor T", HardwareWalletModels.Trezor_T.FriendlyName());
		Assert.Equal("Trezor T Simulator", HardwareWalletModels.Trezor_T_Simulator.FriendlyName());
		Assert.Equal("Trezor Safe 3", HardwareWalletModels.Trezor_Safe_3.FriendlyName());
		Assert.Equal("BitBox", HardwareWalletModels.BitBox02_BTCOnly.FriendlyName());
		Assert.Equal("BitBox", HardwareWalletModels.BitBox02_Multi.FriendlyName());
		Assert.Equal("Jade", HardwareWalletModels.Jade.FriendlyName());
	}
}
