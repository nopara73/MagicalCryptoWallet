using System.IO;
using System.Linq;
using System.Collections.Generic;
using System.Security;
using System.Security.Cryptography;
using System.Text.Json;
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
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Stores;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class SingleWalletTests
{
	internal const string SyntheticMnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
	[Fact]
	public async Task UnknownRecoveryBirthdayStartsAtEarliestSupportedMainnetCheckpointAsync()
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync(), Network.Main);
		var keys = app.NewKeys("secret");
		keys.SetFilePath(null);
		keys.SetMaxBestHeight(0);
		keys.SetFilePath(app.Session.WalletDirectories.NewWalletFilePath);
		var wallet = app.Session.Configure(keys);
		await app.InitializeAsync();
		await WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
		var checkpoint = MagicalCryptoWallet.Blockchain.BlockFilters.FilterCheckpoints.GetCheckpointsByNetwork(Network.Main)[0];
		Assert.Equal((uint)checkpoint.Header.Height, (uint)keys.GetBestHeight());
		Assert.True(wallet.InitialSynchronizationFinished.IsCompletedSuccessfully);
		Assert.True(app.Session.Snapshot.CoinJoinRequiresAuthorization);
	}
	[Fact]
	public async Task FailedStorageRecoveryRemainsFaultedAndCanBeRetriedAsync()
	{
		var directory = new WalletDirectories(Network.RegTest, await Common.GetEmptyWorkDirAsync());
		int attempts = 0;
		var session = new WalletSession(Network.RegTest, directory, _ => throw new InvalidOperationException("No configured wallet in this test."), recoverStorage: _ =>
		{
			if (++attempts == 1) { throw new IOException("Synthetic storage is still unavailable."); }
			return Task.CompletedTask;
		});
		try
		{
			session.ReportInitializationFailure(new IOException("Synthetic storage failure."));
			await session.RetryAsync();
			Assert.Equal(WalletSessionState.Faulted, session.Snapshot.State);
			Assert.Contains("still unavailable", session.Snapshot.Error);
			await session.RetryAsync();
			Assert.Equal(WalletSessionState.Unconfigured, session.Snapshot.State);
			Assert.Equal(2, attempts);
		}
		finally { await session.StopAsync(CancellationToken.None); }
	}
	[Fact]
	public async Task ThrowingObserverDoesNotHideStatusFromOtherSubscribersAsync()
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		app.Session.Configure(app.NewKeys());
		await app.InitializeAsync();
		await WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
		using var failing = app.Session.Subscribe(status => { if (status.State == WalletSessionState.Offline) { throw new InvalidOperationException("Synthetic subscriber failure."); } });
		WalletSessionSnapshot? observed = null;
		using var other = app.Session.Subscribe(status => observed = status);
		app.Connected = false;
		await WaitForAsync(() => observed?.State == WalletSessionState.Offline);
		Assert.Equal(app.Session.Snapshot, observed);
	}
	[Theory]
	[InlineData("../external.tmp", "00")]
	[InlineData(".Wallet.synthetic.tmp", "00")]
	[InlineData(null, null)]
	public async Task MalformedSetupJournalCannotAdoptOrOverwriteFilesAsync(string? temporary, string? hash)
	{
		var directory = new WalletDirectories(Network.RegTest, await Common.GetEmptyWorkDirAsync());
		await File.WriteAllTextAsync(directory.NewWalletFilePath, "synthetic collision");
		await File.WriteAllTextAsync(Path.Combine(directory.WalletsDir, ".wallet-setup"), JsonSerializer.Serialize(new { TemporaryFile = temporary, Sha256 = hash }));
		Assert.Throws<InvalidDataException>(() => directory.ResolveConfiguredWalletFile());
		Assert.Equal("synthetic collision", await File.ReadAllTextAsync(directory.NewWalletFilePath));
		Assert.False(File.Exists(directory.ConfiguredWalletFilePath));
	}
	[Fact]
	public async Task SetupUsesFixedFileAndRejectsConcurrentSecondDraftAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		await using (var app = new SyntheticApplication(root))
		{
			int events = 0;
			app.Session.WalletConfigured += (_, _) => Interlocked.Increment(ref events);
			var drafts = new[] { app.NewKeys(), app.NewKeys() };
			Assert.False(File.Exists(app.Session.WalletDirectories.NewWalletFilePath));
			var outcomes = await Task.WhenAll(drafts.Select(draft => Task.Run(() =>
			{
				try { app.Session.Configure(draft); return true; }
				catch (InvalidOperationException) { return false; }
			})));
			Assert.Single(outcomes, success => success);
			Assert.Equal(1, events);
			Assert.Equal("Wallet", await File.ReadAllTextAsync(app.Session.WalletDirectories.ConfiguredWalletFilePath));
			Assert.Single(Directory.GetFiles(app.Session.WalletDirectories.WalletsDir, "*.json"));
		}
		await using var restarted = new SyntheticApplication(root);
		Assert.Equal("Wallet.json", Path.GetFileName(restarted.Session.GetWallet()!.KeyManager.FilePath));
	}
	[Theory]
	[InlineData(true)]
	[InlineData(false)]
	public async Task LegacyAdoptionPreservesEveryFileAndBecomesPermanentAsync(bool hasPreference)
	{
		string root = await Common.GetEmptyWorkDirAsync();
		var directories = new WalletDirectories(Network.RegTest, root);
		var first = LegacyKeys(directories, "Alpha"); first.ToFile();
		var other = LegacyKeys(directories, "Zulu"); other.ToFile();
		var originals = Directory.GetFiles(directories.WalletsDir).ToDictionary(path => path, File.ReadAllBytes);
		if (hasPreference) { await File.WriteAllTextAsync(Path.Combine(root, "UiConfig.json"), "{\"LastSelectedWallet\":\"Zulu\"}"); }
		var expected = hasPreference ? other.FilePath : first.FilePath;
		await using (var app = new SyntheticApplication(root)) { Assert.Equal(expected, app.Session.GetWallet()!.KeyManager.FilePath); }
		foreach (var (path, content) in originals) { Assert.Equal(content, await File.ReadAllBytesAsync(path)); }
		LegacyKeys(directories, "Aardvark").ToFile();
		await File.WriteAllTextAsync(Path.Combine(root, "UiConfig.json"), "{\"LastSelectedWallet\":\"Aardvark\"}");
		await using var restarted = new SyntheticApplication(root);
		Assert.Equal(expected, restarted.Session.GetWallet()!.KeyManager.FilePath);
		Assert.Equal(3, Directory.GetFiles(directories.WalletsDir, "*.json").Length);
	}
	[Theory]
	[InlineData("Missing")]
	[InlineData("Broken")]
	public async Task ConfiguredFailuresNeverAdoptAnotherFileAndCanBeRetriedAsync(string file)
	{
		string root = await Common.GetEmptyWorkDirAsync();
		var directories = new WalletDirectories(Network.RegTest, root);
		LegacyKeys(directories, "Other").ToFile();
		if (file == "Broken") { await File.WriteAllTextAsync(Path.Combine(directories.WalletsDir, file + ".json"), "invalid"); }
		await File.WriteAllTextAsync(directories.ConfiguredWalletFilePath, file);
		await using var app = new SyntheticApplication(root);
		Assert.Equal(WalletSessionState.Faulted, app.Session.Snapshot.State);
		Assert.Null(app.Session.GetWallet());
		Assert.NotNull(app.Session.Snapshot.Error);
		Assert.Throws<InvalidOperationException>(app.Session.EnsureCanConfigure);
		LegacyKeys(directories, file).ToFile();
		await app.Session.RetryAsync();
		Assert.Equal(Path.Combine(directories.WalletsDir, file + ".json"), app.Session.GetWallet()!.KeyManager.FilePath);
		Assert.Equal(file, await File.ReadAllTextAsync(directories.ConfiguredWalletFilePath));
	}
	[Theory]
	[InlineData("../outside")]
	[InlineData("..\\outside")]
	[InlineData("")]
	[InlineData("CON")]
	[InlineData("wallet.")]
	[InlineData("Wallet:stream")]
	public async Task InvalidMarkerPathsFailClosedAsync(string stem)
	{
		var directories = new WalletDirectories(Network.RegTest, await Common.GetEmptyWorkDirAsync());
		await File.WriteAllTextAsync(directories.ConfiguredWalletFilePath, stem);
		Assert.Throws<InvalidDataException>(() => directories.ResolveConfiguredWalletFile());
	}
	[Fact]
	public async Task ImportIsAnUnsavedDraftAndPreservesSourceAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		var source = LegacyKeys(new WalletDirectories(Network.RegTest, Path.Combine(root, "source")), "Source");
		source.ToFile();
		var original = await File.ReadAllBytesAsync(source.FilePath!);
		await using var app = new SyntheticApplication(Path.Combine(root, "application"));
		var draft = await ImportWalletHelper.ImportWalletAsync(app.Session, source.FilePath!);
		Assert.False(File.Exists(draft.FilePath));
		app.Session.Configure(draft);
		Assert.Equal(source.SegwitExtPubKey, app.Session.GetWallet()!.KeyManager.SegwitExtPubKey);
		await Assert.ThrowsAsync<InvalidOperationException>(() => ImportWalletHelper.ImportWalletAsync(app.Session, source.FilePath!));
		Assert.Equal(original, await File.ReadAllBytesAsync(source.FilePath!));
	}
	[Fact]
	public async Task AFileAppearingDuringSetupIsNeverOverwrittenAsync()
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var keys = app.NewKeys();
		await File.WriteAllTextAsync(keys.FilePath!, "synthetic collision");
		Assert.Throws<InvalidOperationException>(() => app.Session.Configure(keys));
		Assert.Equal("synthetic collision", await File.ReadAllTextAsync(keys.FilePath!));
		Assert.Null(app.Session.GetWallet());
	}
	[Theory]
	[InlineData(false)]
	[InlineData(true)]
	public async Task InterruptedSetupRollsForwardOnlyTheMatchingFileAsync(bool fileWasMoved)
	{
		var directory = new WalletDirectories(Network.RegTest, await Common.GetEmptyWorkDirAsync());
		var temporary = ".Wallet." + Guid.NewGuid().ToString("N") + ".tmp";
		var keys = LegacyKeys(directory, "Temporary");
		keys.SetFilePath(Path.Combine(directory.WalletsDir, temporary)); keys.ToFile();
		var hash = Convert.ToHexString(SHA256.HashData(await File.ReadAllBytesAsync(keys.FilePath!)));
		await File.WriteAllTextAsync(Path.Combine(directory.WalletsDir, ".wallet-setup"), JsonSerializer.Serialize(new { TemporaryFile = temporary, Sha256 = hash }));
		if (fileWasMoved) { File.Move(keys.FilePath!, directory.NewWalletFilePath); }
		Assert.Equal(directory.NewWalletFilePath, directory.ResolveConfiguredWalletFile());
		Assert.Equal("Wallet", await File.ReadAllTextAsync(directory.ConfiguredWalletFilePath));
		Assert.False(File.Exists(Path.Combine(directory.WalletsDir, ".wallet-setup")));
		Assert.Equal(keys.SegwitExtPubKey, KeyManager.FromFile(directory.NewWalletFilePath).SegwitExtPubKey);
	}
	[Fact]
	public async Task EncryptedWalletSynchronizesWithoutAuthorizationOrAWindowAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		await using (var setup = new SyntheticApplication(root)) { setup.Session.Configure(setup.NewKeys("synthetic passphrase")); }
		await using var app = new SyntheticApplication(root);
		await app.InitializeAsync();
		await WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
		Assert.True(app.Session.Snapshot.HasCachedData);
		Assert.Equal(0u, app.Session.Snapshot.SyncHeight);
		Assert.True(app.Session.Snapshot.CoinJoinRequiresAuthorization);
		Assert.Null(app.Session.CoinJoinKeyChain);
		var current = new List<WalletSessionSnapshot>();
		using var subscription = app.Session.Subscribe(current.Add);
		Assert.Equal(app.Session.Snapshot, Assert.Single(current));
		var wallet = app.Session.GetWallet();
		await app.Session.InitializeAsync();
		Assert.Same(wallet, app.Session.GetWallet());
		app.Connected = false;
		await WaitForAsync(() => app.Session.Snapshot.State == WalletSessionState.Offline);
		Assert.True(app.Session.Snapshot.HasCachedData);
		Assert.Throws<InvalidOperationException>(app.Session.EnsureReady);
		app.Connected = true;
		await WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
	}
	[Fact]
	public async Task SetupAfterStoresAreReadyStartsAutomaticallyAsync()
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		await app.InitializeAsync();
		Assert.Equal(WalletSessionState.Unconfigured, app.Session.Snapshot.State);
		app.Session.Configure(app.NewKeys());
		await WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
		Assert.False(app.Session.Snapshot.CoinJoinRequiresAuthorization);
	}
	[Theory]
	[InlineData(false)]
	[InlineData(true)]
	public async Task CancellationWhileWaitingForHeadersStopsCleanlyAsync(bool cancelShutdownWait)
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		app.Session.Configure(app.NewKeys());
		await app.Session.InitializeAsync();
		await WaitForAsync(() => app.Session.Snapshot.HasCachedData);
		Assert.False(app.Session.Snapshot.IsSynchronized);
		using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(10));
		if (cancelShutdownWait) { timeout.Cancel(); }
		var wallet = app.Session.GetWallet()!;
		await app.Session.StopAsync(timeout.Token);
		Assert.Equal(WalletSessionState.Stopping, app.Session.Snapshot.State);
		Assert.True(wallet.WalletFilterProcessor.ExecuteTask is null or { IsCompleted: true });
		Assert.Throws<ObjectDisposedException>(() => { _ = app.Session.InitializeAsync(); });
	}
	[Fact]
	public async Task CoinJoinAuthorizationIsSeparateAndDoesNotSurviveRestartAsync()
	{
		string root = await Common.GetEmptyWorkDirAsync();
		await using (var app = new SyntheticApplication(root))
		{
			app.Session.Configure(app.NewKeys("secret"));
			app.Session.AuthorizeCoinJoin("secret");
			Assert.NotNull(app.Session.CoinJoinKeyChain);
			Assert.Throws<SecurityException>(() => app.Session.AuthorizeCoinJoin("wrong"));
			Assert.Throws<SecurityException>(() => WalletAuthorization.Create(app.Session.GetWallet()!.KeyManager, "wrong"));
			using var operation = WalletAuthorization.Create(app.Session.GetWallet()!.KeyManager, "secret");
			Assert.True(operation.VerifyRecoveryWords(new Mnemonic(SyntheticMnemonic)));
			operation.Dispose();
			Assert.Throws<ObjectDisposedException>(() => operation.MasterKey);
		}
		await using var restarted = new SyntheticApplication(root);
		Assert.Null(restarted.Session.CoinJoinKeyChain);
		Assert.True(restarted.Session.Snapshot.CoinJoinRequiresAuthorization);
	}
	[Fact]
	public async Task SetupRejectsWrongPasswordBeforeWritingAndRetainsTheCorrectAuthorizationAsync()
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var keys = app.NewKeys("secret");
		Assert.Throws<SecurityException>(() => app.Session.Configure(keys, "wrong"));
		Assert.Equal(WalletSessionState.Unconfigured, app.Session.Snapshot.State);
		Assert.False(File.Exists(app.Session.WalletDirectories.NewWalletFilePath));
		app.Session.Configure(keys, "secret");
		Assert.False(app.Session.Snapshot.CoinJoinRequiresAuthorization);
		Assert.NotNull(app.Session.CoinJoinKeyChain);
		Assert.Throws<SecurityException>(() => WalletAuthorization.Create(keys, "wrong"));
		await app.InitializeAsync();
		await WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
	}
	[Theory]
	[InlineData(false)]
	[InlineData(true)]
	public async Task SetupPasswordAuthorizesCoinJoinOnlyForTheCurrentRunAsync(bool recover)
	{
		string root = await Common.GetEmptyWorkDirAsync();
		await using (var app = new SyntheticApplication(root))
		{
			var generator = new WalletGenerator(app.Session.WalletDirectories.WalletsDir, Network.RegTest);
			var keys = generator.GenerateDraft("secret", recover ? new Mnemonic(SyntheticMnemonic) : null).KeyManager;
			app.Session.Configure(keys, "secret");
			Assert.False(app.Session.Snapshot.CoinJoinRequiresAuthorization);
			Assert.NotNull(app.Session.CoinJoinKeyChain);
			Assert.Throws<SecurityException>(() => WalletAuthorization.Create(app.Session.GetWallet()!.KeyManager, "wrong"));
			await app.InitializeAsync();
			await WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
		}
		await using var restarted = new SyntheticApplication(root);
		Assert.Null(restarted.Session.CoinJoinKeyChain);
		Assert.True(restarted.Session.Snapshot.CoinJoinRequiresAuthorization);
	}
	[Fact]
	public async Task SessionReportsUnconfiguredBeforeSetupAsync()
	{
		await using var app = new SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		Assert.Equal(WalletSessionState.Unconfigured, app.Session.Snapshot.State);
		Assert.False(app.Session.Snapshot.HasCachedData);
		Assert.Null(app.Session.GetWallet());
		Assert.Null(app.Session.Snapshot.SyncHeight);
	}
	[Fact]
	public void SelectionCliOptionsAreRejected()
	{
		Assert.Throws<ArgumentException>(() => new Config(PersistentConfigManager.DefaultRegTestConfig, ["--wallet=First"]));
		Assert.Throws<ArgumentException>(() => new Config(PersistentConfigManager.DefaultRegTestConfig, ["--WALLET=First"]));
	}
	[Fact]
	public async Task ExplicitDataDirectoriesHaveIndependentSessionsAsync()
	{
		var root = await Common.GetEmptyWorkDirAsync();
		await using var first = new SyntheticApplication(Path.Combine(root, "first"));
		await using var second = new SyntheticApplication(Path.Combine(root, "second"));
		first.Session.Configure(first.NewKeys()); second.Session.Configure(second.NewKeys());
		Assert.NotSame(first.Session.GetWallet(), second.Session.GetWallet());
		Assert.NotEqual(first.Session.GetWallet()!.KeyManager.FilePath, second.Session.GetWallet()!.KeyManager.FilePath);
	}
	internal static async Task WaitForAsync(Func<bool> condition)
	{
		using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(15));
		while (!condition()) { await Task.Delay(25, timeout.Token); }
	}
	private static KeyManager LegacyKeys(WalletDirectories directories, string stem)
	{
		var keys = KeyManager.CreateNew(new Mnemonic(SyntheticMnemonic), "", Network.RegTest);
		keys.SetFilePath(Path.Combine(directories.WalletsDir, stem + ".json"));
		return keys;
	}
	internal sealed class SyntheticApplication : IAsyncDisposable
	{
		private readonly AllTransactionStore _transactions;
		private readonly FilterStore _filters;
		private readonly MailboxProcessor<CpfpInfoMessage> _cpfp;
		public SyntheticApplication(string root, Network? network = null)
		{
			network ??= Network.RegTest;
			var config = new Config(PersistentConfigManager.DefaultRegTestConfig with { UseTor = "Disabled" }, []);
			Events = new EventBus();
			Headers = new FilterHeaderChain();
			_transactions = new AllTransactionStore(SqliteStorageHelper.InMemoryDatabase, network);
			_filters = new FilterStore(Path.Combine(root, "filters"), network, Headers, Events);
			_cpfp = new MailboxProcessor<CpfpInfoMessage>("SyntheticCpFp", async (mailbox, cancel) =>
			{
				var handler = CpfpInfoUpdater.CreateForRegTest();
				while (!cancel.IsCancellationRequested) { await handler(await mailbox.ReceiveAsync(cancel), Unit.Instance, cancel); }
			});
			_cpfp.Start();
			BlockProvider blocks = (_, _) => throw new InvalidOperationException("No block requests are allowed in this test.");
			var factory = MagicalCryptoWallet.Wallets.Wallet.CreateFactory(network, _filters, _transactions, Headers, new MempoolService(Events), config.ServiceConfiguration, blocks, Events, new CpfpInfoProvider(_cpfp));
			Session = new WalletSession(network, new WalletDirectories(network, root), factory, () => Connected);
		}
		public WalletSession Session { get; }
		public EventBus Events { get; }
		public FilterHeaderChain Headers { get; }
		public bool Connected { get; set; } = true;
		public KeyManager NewKeys(string password = "") => new WalletGenerator(Session.WalletDirectories.WalletsDir, Session.Network).GenerateDraft(password, new Mnemonic(SyntheticMnemonic)).KeyManager;
		public async Task InitializeAsync()
		{
			await _transactions.InitializeAsync();
			await _filters.InitializeAsync(new Height.ChainHeight(0), CancellationToken.None);
			await Session.InitializeAsync();
		}
		public async ValueTask DisposeAsync()
		{
			await Session.StopAsync(CancellationToken.None);
			_cpfp.Dispose(); _filters.Dispose(); _transactions.Dispose();
		}
	}
}
