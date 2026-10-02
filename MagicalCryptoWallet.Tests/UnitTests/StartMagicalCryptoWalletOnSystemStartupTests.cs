using Microsoft.Win32;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Threading.Tasks;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Tests.Helpers;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class StartMagicalCryptoWalletOnSystemStartupTests
{
	[Theory]
	[InlineData(null)]
	[InlineData("\"C:\\Program Files\\MagicalCryptoWallet\\magicalcryptowallet.exe\" startsilent")]
	public void ModifyWindowsStartupPreservesExistingEntries(string? existingCommand)
	{
		if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
		{
			return;
		}

		// Never register the test executable in the developer's actual startup settings.
		string keyPath = $"SOFTWARE\\MagicalCryptoWallet.Tests\\Startup\\{Guid.NewGuid():N}";
		try
		{
			using RegistryKey key = Registry.CurrentUser.CreateSubKey(keyPath);
			key.SetValue("OtherApplication", "unchanged");
			if (existingCommand is not null)
			{
				key.SetValue(nameof(MagicalCryptoWallet), existingCommand);
			}

			string expectedCommand = existingCommand ?? $"\"{EnvironmentHelpers.GetExecutablePath()}\" {StartupHelper.SilentArgument}";
			WindowsStartupHelper.AddOrRemoveRegistryKey(true, keyPath);
			Assert.Equal(expectedCommand, key.GetValue(nameof(MagicalCryptoWallet)));

			WindowsStartupHelper.AddOrRemoveRegistryKey(true, keyPath);
			Assert.Equal(expectedCommand, key.GetValue(nameof(MagicalCryptoWallet)));

			WindowsStartupHelper.AddOrRemoveRegistryKey(false, keyPath);
			Assert.Null(key.GetValue(nameof(MagicalCryptoWallet)));

			WindowsStartupHelper.AddOrRemoveRegistryKey(false, keyPath);
			Assert.Null(key.GetValue(nameof(MagicalCryptoWallet)));
			Assert.Equal("unchanged", key.GetValue("OtherApplication"));
		}
		finally
		{
			Registry.CurrentUser.DeleteSubKeyTree(keyPath, throwOnMissingSubKey: false);
		}
	}

	[Fact]
	public async Task StartupFilesUseIndependentIdentityAndPreserveOtherApplicationsAsync()
	{
		string home = await Common.GetEmptyWorkDirAsync();
		string autostart = Path.Combine(home, ".config", "autostart");
		Directory.CreateDirectory(autostart);
		string existing = Path.Combine(autostart, "other-wallet.desktop");
		await File.WriteAllTextAsync(existing, "existing application");
		await LinuxStartupHelper.AddOrRemoveDesktopFileAsync(true, home);
		string desktop = Path.Combine(autostart, Constants.ApplicationId + ".desktop");
		Assert.Contains("Name=Magical Crypto Wallet", await File.ReadAllTextAsync(desktop));
		Assert.Contains($"Exec=\"{EnvironmentHelpers.GetExecutablePath()}\" startsilent", await File.ReadAllTextAsync(desktop));
		await LinuxStartupHelper.AddOrRemoveDesktopFileAsync(false, home);
		Assert.False(File.Exists(desktop));
		Assert.Equal("existing application", await File.ReadAllTextAsync(existing));

		await MacOsStartupHelper.AddOrRemoveStartupItemAsync(true, home);
		string plist = Path.Combine(home, "Library", "LaunchAgents", Constants.SilentPlistName);
		var xml = System.Xml.Linq.XDocument.Load(plist);
		Assert.Contains(Constants.ApplicationId + ".startup", xml.Descendants("string").Select(x => x.Value));
		Assert.Contains(EnvironmentHelpers.GetExecutablePath(), xml.Descendants("string").Select(x => x.Value));
		await MacOsStartupHelper.AddOrRemoveStartupItemAsync(false, home);
		Assert.False(File.Exists(plist));
	}

	[Fact]
	public async Task RunOnSystemStartupGetsSetCorrectlyAsync()
	{
		// Imitate fresh UiConfig file.
		string workDir = await Common.GetEmptyWorkDirAsync();

		UiConfig config = UiConfig.LoadFile(Path.Combine(workDir, "UiConfig.json"));
		Assert.True(config.Oobe);
		Assert.False(config.RunOnSystemStartup);
	}

	private UiConfig GetUiConfig()
	{
		string dataDir = EnvironmentHelpers.GetDataDir(Path.Combine("MagicalCryptoWallet", "Client"));
		return UiConfig.LoadFile(Path.Combine(dataDir, "UiConfig.json"));
	}
}
