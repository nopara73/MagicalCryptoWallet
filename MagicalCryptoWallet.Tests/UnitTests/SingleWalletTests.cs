using System.IO;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using Newtonsoft.Json.Linq;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.Blocks;
using MagicalCryptoWallet.Blockchain.Mempool;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Client.Rpc;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Rpc;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Stores;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class SingleWalletTests
{
	private const string SyntheticMnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";

	[Fact]
	public async Task FirstWalletSurvivesRestartAndSecondWalletIsRejectedAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		await using (var app = new SyntheticApplication(root))
		{
			Assert.Null(app.Manager.GetWallet());
			var wallet = app.Manager.AddWallet(app.NewKeys("First"));
			var original = await File.ReadAllBytesAsync(wallet.KeyManager.FilePath!);
			Assert.Throws<InvalidOperationException>(() => app.Manager.AddWallet(app.NewKeys("Second")));
			Assert.False(File.Exists(app.Manager.WalletDirectories.GetWalletFilePaths("Second")));
			Assert.Equal(original, await File.ReadAllBytesAsync(wallet.KeyManager.FilePath!));
			Assert.Equal("First", await File.ReadAllTextAsync(app.Manager.WalletDirectories.ConfiguredWalletFilePath));
		}
		await using var restarted = new SyntheticApplication(root);
		Assert.Equal("First", restarted.Manager.GetWallet()!.WalletName);
		Assert.Single(restarted.Manager.WalletDirectories.EnumerateWalletFiles());
	}

	[Fact]
	public async Task ConcurrentCreationCommitsExactlyOneWalletAsync()
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var first = app.NewKeys("First");
		var second = app.NewKeys("Second");
		int events = 0;
		app.Manager.WalletAdded += (_, _) => Interlocked.Increment(ref events);
		var results = await Task.WhenAll(new[] { first, second }.Select(keys => Task.Run(() =>
		{
			try
			{
				app.Manager.AddWallet(keys);
				return true;
			}
			catch (InvalidOperationException)
			{
				return false;
			}
		})));
		Assert.Single(results, success => success);
		Assert.Equal(1, events);
		Assert.Single(app.Manager.WalletDirectories.EnumerateWalletFiles());
		Assert.Equal(app.Manager.GetWallet()!.WalletName, await File.ReadAllTextAsync(app.Manager.WalletDirectories.ConfiguredWalletFilePath));
	}

	[Fact]
	public async Task ExistingInstallationAdoptsLastUsedWalletWithoutChangingFilesAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		var directories = new WalletDirectories(Network.RegTest, root);
		var first = NewKeys(directories, "Alpha");
		var lastUsed = NewKeys(directories, "Zulu");
		first.ToFile();
		lastUsed.ToFile();
		var originalFirst = await File.ReadAllBytesAsync(first.FilePath!);
		var originalLastUsed = await File.ReadAllBytesAsync(lastUsed.FilePath!);
		await File.WriteAllTextAsync(Path.Combine(root, "UiConfig.json"), "{\"LastSelectedWallet\":\"Zulu\"}");
		await using (var app = new SyntheticApplication(root))
		{
			Assert.Equal("Zulu", app.Manager.GetWallet()!.WalletName);
			Assert.Equal(originalFirst, await File.ReadAllBytesAsync(first.FilePath!));
			Assert.Equal(originalLastUsed, await File.ReadAllBytesAsync(lastUsed.FilePath!));
		}
		// Changing the old preference or adding another file cannot change the adopted identity.
		await File.WriteAllTextAsync(Path.Combine(root, "UiConfig.json"), "{\"LastSelectedWallet\":\"Alpha\"}");
		NewKeys(directories, "Aardvark").ToFile();
		await using var restarted = new SyntheticApplication(root);
		Assert.Equal("Zulu", restarted.Manager.GetWallet()!.WalletName);
		Assert.Equal(3, directories.EnumerateWalletFiles().Count());
	}

	[Fact]
	public async Task ExistingInstallationWithoutPreferenceUsesStableNameOrderAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		var directories = new WalletDirectories(Network.RegTest, root);
		NewKeys(directories, "Zulu").ToFile();
		NewKeys(directories, "Alpha").ToFile();
		await using var app = new SyntheticApplication(root);
		Assert.Equal("Alpha", app.Manager.GetWallet()!.WalletName);
	}

	[Fact]
	public async Task MissingConfiguredWalletDoesNotOpenAnotherFileAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		var directories = new WalletDirectories(Network.RegTest, root);
		NewKeys(directories, "Other").ToFile();
		directories.SetConfiguredWalletName("Missing");
		Assert.Throws<FileNotFoundException>(() => new WalletManager(Network.RegTest, directories, _ => throw new Exception("No other wallet should be loaded.")));
		Assert.Single(directories.EnumerateWalletFiles());
	}

	[Fact]
	public async Task CorruptConfiguredWalletDoesNotOpenAnotherFileAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		var directories = new WalletDirectories(Network.RegTest, root);
		NewKeys(directories, "Other").ToFile();
		await File.WriteAllTextAsync(directories.GetWalletFilePaths("Broken"), "invalid wallet");
		directories.SetConfiguredWalletName("Broken");
		Assert.ThrowsAny<Exception>(() => new WalletManager(Network.RegTest, directories, _ => throw new Exception("No other wallet should be loaded.")));
		Assert.Equal("Broken", await File.ReadAllTextAsync(directories.ConfiguredWalletFilePath));
	}

	[Theory]
	[InlineData("../outside")]
	[InlineData("..\\outside")]
	[InlineData("")]
	public async Task InvalidConfiguredPathsAreRejectedAsync(string name)
	{
		var directories = new WalletDirectories(Network.RegTest, await Common.GetEmptyWorkDirAsync());
		await File.WriteAllTextAsync(directories.ConfiguredWalletFilePath, name);
		Assert.Throws<InvalidDataException>(() => directories.GetConfiguredWalletName());
	}

	[Fact]
	public async Task RenameUpdatesThePersistentIdentityAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		await using (var app = new SyntheticApplication(root))
		{
			var wallet = app.Manager.AddWallet(app.NewKeys("Before"));
			app.Manager.RenameWallet(wallet, "After");
			Assert.Equal("After", wallet.WalletName);
			Assert.False(File.Exists(app.Manager.WalletDirectories.GetWalletFilePaths("Before")));
			Assert.Equal("After", await File.ReadAllTextAsync(app.Manager.WalletDirectories.ConfiguredWalletFilePath));
		}
		await using var restarted = new SyntheticApplication(root);
		Assert.Equal("After", restarted.Manager.GetWallet()!.WalletName);
	}

	[Fact]
	public async Task ImportIsAvailableOnlyBeforeSetupAndPreservesTheSourceAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		var sourceDirectories = new WalletDirectories(Network.RegTest, Path.Combine(root, "source"));
		var source = NewKeys(sourceDirectories, "Source");
		source.ToFile();
		byte[] original = await File.ReadAllBytesAsync(source.FilePath!);
		await using var app = new SyntheticApplication(Path.Combine(root, "application"));
		var imported = await ImportWalletHelper.ImportWalletAsync(app.Manager, "Imported", source.FilePath!);
		Assert.Empty(app.Manager.WalletDirectories.EnumerateWalletFiles());
		app.Manager.AddWallet(imported);
		Assert.Equal(source.SegwitExtPubKey, app.Manager.GetWallet()!.KeyManager.SegwitExtPubKey);
		await Assert.ThrowsAsync<InvalidOperationException>(() => ImportWalletHelper.ImportWalletAsync(app.Manager, "Extra", source.FilePath!));
		Assert.False(File.Exists(app.Manager.WalletDirectories.GetWalletFilePaths("Extra")));
		Assert.Equal(original, await File.ReadAllBytesAsync(source.FilePath!));
	}

	[Fact]
	public async Task DraftCreationDoesNotSaveAnUnacceptedWalletAsync()
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var generator = new WalletGenerator(app.Manager.WalletDirectories.WalletsDir, Network.RegTest);
		var first = generator.GenerateWallet("First", "", new Mnemonic(SyntheticMnemonic), toFile: false).KeyManager;
		var second = generator.GenerateWallet("Second", "", new Mnemonic(SyntheticMnemonic), toFile: false).KeyManager;
		Assert.Empty(app.Manager.WalletDirectories.EnumerateWalletFiles());
		app.Manager.AddWallet(first);
		Assert.Throws<InvalidOperationException>(() => app.Manager.AddWallet(second));
		Assert.Single(app.Manager.WalletDirectories.EnumerateWalletFiles());
	}

	[Fact]
	public async Task FilesCreatedDuringSetupAreNeverOverwrittenAsync()
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var keys = app.NewKeys("Collision");
		await File.WriteAllTextAsync(keys.FilePath!, "synthetic existing file");
		Assert.Throws<InvalidOperationException>(() => app.Manager.AddWallet(keys));
		Assert.Equal("synthetic existing file", await File.ReadAllTextAsync(keys.FilePath!));
		Assert.Null(app.Manager.GetWallet());
	}

	[Fact]
	public async Task WalletNamesEndingInJsonRetainTheirIdentityAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		await using (var app = new SyntheticApplication(root))
		{
			var generator = new WalletGenerator(app.Manager.WalletDirectories.WalletsDir, Network.RegTest);
			app.Manager.AddWallet(generator.GenerateWallet("Personal.json", "", new Mnemonic(SyntheticMnemonic), toFile: false).KeyManager);
			Assert.Equal("Personal.json", app.Manager.GetWallet()!.WalletName);
		}
		await using var restarted = new SyntheticApplication(root);
		Assert.Equal("Personal.json", restarted.Manager.GetWallet()!.WalletName);
	}

	[Fact]
	public async Task RpcUsesTheRootWalletAndRejectsSelectionAndExtraCreationAsync()
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var service = new MagicalCryptoWalletJsonRpcService(app.Global);
		var handler = new JsonRpcRequestHandler<MagicalCryptoWalletJsonRpcService>(service, Network.RegTest);
		service.RecoverWallet("Recovered", SyntheticMnemonic);
		var info = JObject.Parse(await handler.HandleAsync("/", "{\"jsonrpc\":\"2.0\",\"id\":\"1\",\"method\":\"getwalletinfo\"}", CancellationToken.None));
		Assert.Equal("Recovered", info["result"]!["walletName"]!.Value<string>());
		Assert.Throws<InvalidOperationException>(() => service.Initialize("/Recovered", true));
		Assert.Throws<InvalidOperationException>(() => service.CreateWallet("Extra", ""));
		Assert.Throws<InvalidOperationException>(() => service.RecoverWallet("Extra", SyntheticMnemonic));
		Assert.Single(app.Manager.WalletDirectories.EnumerateWalletFiles());
		foreach (var removedMethod in new[] { "listwallets", "startcoinjoinsweep" })
		{
			var result = JObject.Parse(await handler.HandleAsync("/", $"{{\"jsonrpc\":\"2.0\",\"id\":\"1\",\"method\":\"{removedMethod}\"}}", CancellationToken.None));
			Assert.Equal((int)JsonRpcErrorCodes.MethodNotFound, result["error"]!["code"]!.Value<int>());
		}
		var metadata = new JsonRpcServiceMetadataProvider(typeof(MagicalCryptoWalletJsonRpcService));
		Assert.True(metadata.TryGetMetadata("loadwallet", out var load));
		Assert.Empty(load.Parameters);
		foreach (var parameters in new[] { "[\"Recovered\"]", "{\"walletName\":\"Recovered\"}" })
		{
			var result = JObject.Parse(await handler.HandleAsync("/", $"{{\"jsonrpc\":\"2.0\",\"id\":\"1\",\"method\":\"loadwallet\",\"params\":{parameters}}}", CancellationToken.None));
			Assert.Equal((int)JsonRpcErrorCodes.InvalidParams, result["error"]!["code"]!.Value<int>());
		}
	}

	[Fact]
	public void NamedWalletCliOptionsAreRejected()
	{
		Assert.Throws<ArgumentException>(() => new Config(PersistentConfigManager.DefaultRegTestConfig, ["--wallet=First", "--wallet=Second"]));
		Assert.Throws<ArgumentException>(() => new Config(PersistentConfigManager.DefaultRegTestConfig, ["--WALLET=First"]));
	}

	[Fact]
	public async Task SeparateDataDirectoriesRemainIndependentAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		await using var first = new SyntheticApplication(Path.Combine(root, "first"));
		await using var second = new SyntheticApplication(Path.Combine(root, "second"));
		first.Manager.AddWallet(first.NewKeys("First"));
		second.Manager.AddWallet(second.NewKeys("Second"));
		Assert.Equal("First", first.Manager.GetWallet()!.WalletName);
		Assert.Equal("Second", second.Manager.GetWallet()!.WalletName);
	}

	private static KeyManager NewKeys(WalletDirectories directories, string name)
	{
		var key = ExtKey.CreateFromSeed(new byte[32]);
		var keys = KeyManager.CreateNewHardwareWalletWatchOnly(key.Neuter().PubKey.GetHDFingerPrint(), key.Neuter(), null, null, null, Network.RegTest);
		keys.SetFilePath(directories.GetWalletFilePaths(name + ".json"));
		return keys;
	}

	internal sealed class SyntheticApplication : IAsyncDisposable
	{
		private readonly AllTransactionStore _transactions;
		private readonly FilterStore _filters;
		private readonly MailboxProcessor<CpfpInfoMessage> _cpfp;

		public SyntheticApplication(string root)
		{
			// Real wallet, transaction, and filter services; no host, workers, networking or real funds.
			var config = new Config(PersistentConfigManager.DefaultRegTestConfig with { UseTor = "Disabled" }, []);
			var events = new EventBus();
			var headers = new FilterHeaderChain();
			_transactions = new AllTransactionStore(SqliteStorageHelper.InMemoryDatabase, Network.RegTest);
			_filters = new FilterStore(Path.Combine(root, "filters"), Network.RegTest, headers, events);
			_cpfp = new MailboxProcessor<CpfpInfoMessage>("SyntheticCpFp", (_, _) => Task.CompletedTask);
			BlockProvider blocks = (_, _) => throw new InvalidOperationException("No block requests are allowed in this test.");
			var factory = MagicalCryptoWallet.Wallets.Wallet.CreateFactory(Network.RegTest, _filters, _transactions, headers, new MempoolService(events), config.ServiceConfiguration, blocks, events, new CpfpInfoProvider(_cpfp));
			Manager = new WalletManager(Network.RegTest, new WalletDirectories(Network.RegTest, root), factory);
			Global = (Global)RuntimeHelpers.GetUninitializedObject(typeof(Global));
			SetGlobalProperty(nameof(Global.DataDir), root);
			SetGlobalProperty(nameof(Global.Config), config);
			SetGlobalProperty(nameof(Global.FilterHeaders), headers);
			SetGlobalProperty(nameof(Global.WalletManager), Manager);
		}
		public Global Global { get; }
		public WalletManager Manager { get; }
		public KeyManager NewKeys(string name) => SingleWalletTests.NewKeys(Manager.WalletDirectories, name);
		public async ValueTask DisposeAsync()
		{
			await Manager.RemoveAndStopAsync(CancellationToken.None);
			_cpfp.Dispose();
			_filters.Dispose();
			_transactions.Dispose();
		}
		private void SetGlobalProperty(string name, object value) =>
			typeof(Global).GetField($"<{name}>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(Global, value);
	}
}
