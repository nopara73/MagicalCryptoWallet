using System;
using System.Diagnostics;
using System.IO;
using System.Text.Json;
using System.Threading.Tasks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Wallets;
using NBitcoin;
using Xunit;

namespace MagicalCryptoWallet.IntegrationTests.WalletTests;

[Collection("Integration tests")]
public class DesktopLifecycleTests(ITestOutputHelper output)
{
	[Fact(Timeout = 900_000)]
	public async Task PackagedWindowsDesktopSynchronizesHiddenActivatesAndQuitsAsync()
	{
		var package = Environment.GetEnvironmentVariable("MCW_DESKTOP_PACKAGE");
		Assert.SkipWhen(!OperatingSystem.IsWindows() || string.IsNullOrEmpty(package), "Requires the Windows desktop package produced by CI.");
		var root = new DirectoryInfo(AppContext.BaseDirectory);
		while (root is not null && !File.Exists(Path.Combine(root.FullName, "Contrib", "Tests", "test-single-wallet-process.py"))) { root = root.Parent; }
		Assert.NotNull(root);
		var run = Path.Combine(root.FullName, ".artifacts", "desktop-lifecycle", "synthetic " + Guid.NewGuid().ToString("N"));
		Directory.CreateDirectory(run);
		void Stage(string name) => File.AppendAllText(Path.Combine(run, "fixture-stage.log"), $"{DateTimeOffset.UtcNow:O} {name}{Environment.NewLine}");
		Stage("synthetic setup started");
		// SQLite's native Windows path limit is independent of .NET long-path support.
		var data = Path.Combine(Path.GetTempPath(), "MCW synthetic lifecycle " + Guid.NewGuid().ToString("N"));
		Directory.CreateDirectory(data);
		var directories = new WalletDirectories(Network.RegTest, data);
		Stage("creating synthetic encrypted wallet");
		var keys = KeyManager.CreateNew(new Mnemonic("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"),
			"synthetic lifecycle password", Network.RegTest, directories.NewWalletFilePath);
		Stage("synthetic encrypted wallet created");
		var address = keys.GetNextReceiveKey("synthetic funding").GetP2wpkhAddress(Network.RegTest);
		Stage("saving synthetic wallet");
		keys.ToFile();
		Stage("synthetic wallet saved");
		await File.WriteAllTextAsync(directories.ConfiguredWalletFilePath, Path.GetFileNameWithoutExtension(keys.FilePath));
		PersistentConfigManager.ToFile(Path.Combine(data, "Config.RegTest.json"), PersistentConfigManager.DefaultRegTestConfig with
		{
			CoordinatorUri = "", UseTor = "Disabled", FeeRateEstimationProvider = "None", ExchangeRateProvider = "None",
			EnableGpu = false, DownloadNewVersion = false
		});
		await File.WriteAllTextAsync(Path.Combine(data, "UiConfig.json"), JsonSerializer.Serialize(new
		{
			Oobe = false, LastVersionHighlightsDisplayed = "99.99.99.0", WindowState = "Normal",
			Autocopy = false, AutoPaste = false, IsCustomChangeAddress = false, PrivacyMode = true, DarkModeEnabled = true,
			RunOnSystemStartup = false, HideOnClose = true, SendAmountConversionReversed = false, WindowWidth = 1100, WindowHeight = 760
		}));
		await File.WriteAllTextAsync(Path.Combine(data, "retained-user-script.scm"), "; synthetic user-written script must remain intact\n(+ 1 2)\n");
		await File.WriteAllTextAsync(Path.Combine(data, "lifecycle-seed.json"), JsonSerializer.Serialize(new
		{
			receiveAddress = address.ToString(), walletFile = Path.GetRelativePath(data, keys.FilePath!), resyncHeightMargin = Constants.ResyncHeightMargin
		}));
		var bitcoin = Path.Combine(AppContext.BaseDirectory, "BundledApps", "Binaries", "win-x64", "bitcoind.exe");
		Assert.True(File.Exists(bitcoin), "Integration tests must provide their isolated Bitcoin Core binary.");
		Stage("launching packaged desktop harness");
		var start = new ProcessStartInfo("python") { WorkingDirectory = root.FullName, RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false, CreateNoWindow = true };
		start.Environment["MCW_LIFECYCLE_TRACE"] = Path.Combine(run, "harness-stack.log");
		foreach (var argument in new[] { "Contrib/Tests/test-single-wallet-process.py", "--package", Path.GetFullPath(package!), "--data-dir", data, "--bitcoind", bitcoin, "--output", run }) { start.ArgumentList.Add(argument); }
		using var process = Process.Start(start)!;
		// The harness never consumes input and must not inherit the test host's
		// private application-service pipe during Python console initialization.
		process.StandardInput.Close();
		var stdout = process.StandardOutput.ReadToEndAsync();
		var stderr = process.StandardError.ReadToEndAsync();
		try { await process.WaitForExitAsync(TestContext.Current.CancellationToken); }
		finally
		{
			if (!process.HasExited)
			{
				process.Kill(entireProcessTree: true);
				await process.WaitForExitAsync();
			}
			await File.WriteAllTextAsync(Path.Combine(run, "harness-stdout.log"), await stdout);
			await File.WriteAllTextAsync(Path.Combine(run, "harness-stderr.log"), await stderr);
			Stage($"packaged desktop harness exited {process.ExitCode}");
		}
		output.WriteLine(await stdout);
		output.WriteLine(await stderr);
		Assert.Equal(0, process.ExitCode);
	}
}
