using System.Collections.Generic;
using System.Diagnostics.CodeAnalysis;
using System.IO;
using System.Linq;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using Newtonsoft.Json.Linq;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Client.Rpc;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class SoftwareWalletTests
{
	public static IEnumerable<object[]> InvalidFiles =>
	[
		["hardware", true], ["watch-only", true], ["skeleton", true], ["missing-secret", true],
		["missing-chain", false], ["null-chain", false], ["short-chain", false], ["long-chain", false], ["invalid-chain", false]
	];

	[Theory]
	[MemberData(nameof(InvalidFiles))]
	public async Task InvalidImportsPreserveSourceAndLeaveSetupUnconfiguredAsync(string kind, bool unsupported)
	{
		var root = await Common.GetEmptyWorkDirAsync();
		var source = Path.Combine(root, "source.json");
		await WriteInvalidFileAsync(source, kind);
		var original = await File.ReadAllBytesAsync(source);
		var directories = new WalletDirectories(Network.RegTest, Path.Combine(root, "destination"));
		var session = new WalletSession(Network.RegTest, directories, _ => throw new InvalidOperationException("An invalid import must never initialize a wallet."));
		try
		{
			var error = await Record.ExceptionAsync(() => ImportWalletHelper.ImportWalletAsync(session, source));
			AssertInvalidError(error, unsupported);
			Assert.Equal(original, await File.ReadAllBytesAsync(source));
			Assert.Empty(Directory.GetFiles(directories.WalletsDir));
			Assert.Equal(WalletSessionState.Unconfigured, session.Snapshot.State);
			Assert.Null(session.GetWallet());
		}
		finally { await session.StopAsync(CancellationToken.None); }
	}

	[Theory]
	[MemberData(nameof(InvalidFiles))]
	public async Task InvalidConfiguredFilesAndRetryCannotAdoptOtherKeysAsync(string kind, bool unsupported)
	{
		foreach (bool hasMarker in new[] { false, true })
		{
			var root = await Common.GetEmptyWorkDirAsync();
			var directories = new WalletDirectories(Network.RegTest, root);
			await WriteInvalidFileAsync(Path.Combine(directories.WalletsDir, "AUnsupported.json"), kind);
			_ = KeyManager.CreateNew(new Mnemonic(SingleWalletTests.SyntheticMnemonic), "secret", Network.RegTest, Path.Combine(directories.WalletsDir, "ZOther.json"));
			if (hasMarker) { await File.WriteAllTextAsync(directories.ConfiguredWalletFilePath, "AUnsupported"); }
			var originals = ReadFiles(directories.WalletsDir);
			await using var app = new SingleWalletTests.SyntheticApplication(root);
			Assert.Equal(WalletSessionState.Faulted, app.Session.Snapshot.State);
			Assert.Null(app.Session.GetWallet());
			Assert.Contains(unsupported ? "no local private keys" : "32 bytes", app.Session.Snapshot.Error);
			Assert.Throws<InvalidOperationException>(() => app.Session.Configure(app.NewKeys()));
			await app.InitializeAsync();
			await app.Session.RetryAsync();
			Assert.Equal(WalletSessionState.Faulted, app.Session.Snapshot.State);
			Assert.Null(app.Session.GetWallet());
			AssertFilesUnchanged(directories.WalletsDir, originals);
		}
	}

	[Theory]
	[MemberData(nameof(InvalidFiles))]
	public async Task InterruptedInvalidSetupPreservesCandidateMarkerAndJournalAsync(string kind, bool unsupported)
	{
		foreach (bool alreadyMoved in new[] { false, true })
		{
			var directories = new WalletDirectories(Network.RegTest, await Common.GetEmptyWorkDirAsync());
			var temporary = ".Wallet." + Guid.NewGuid().ToString("N") + ".tmp";
			var candidate = alreadyMoved ? directories.NewWalletFilePath : Path.Combine(directories.WalletsDir, temporary);
			await WriteInvalidFileAsync(candidate, kind);
			var hash = Convert.ToHexString(SHA256.HashData(await File.ReadAllBytesAsync(candidate)));
			await File.WriteAllTextAsync(directories.SetupJournalPath, JsonSerializer.Serialize(new { TemporaryFile = temporary, Sha256 = hash }));
			if (alreadyMoved) { await File.WriteAllTextAsync(directories.ConfiguredWalletFilePath, "Wallet"); }
			var originals = ReadFiles(directories.WalletsDir);
			AssertInvalidError(Record.Exception(() => directories.ResolveConfiguredWalletFile()), unsupported);
			AssertFilesUnchanged(directories.WalletsDir, originals);
		}
	}

	[Theory]
	[InlineData(0)]
	[InlineData(31)]
	[InlineData(33)]
	public async Task ConstructorRejectsInvalidChainCodeBeforeWritingAsync(int length)
	{
		var path = Path.Combine(await Common.GetEmptyWorkDirAsync(), "invalid.json");
		var keys = NewKeys();
		Assert.Throws<InvalidDataException>(() => new KeyManager(keys.EncryptedSecret, new byte[length], keys.MasterFingerprint,
			keys.SegwitExtPubKey, keys.TaprootExtPubKey,
			21, new BlockchainState(Network.RegTest), path));
		Assert.False(File.Exists(path));
	}

	[Fact]
	public async Task LegacySoftwarePreferencesAreIgnoredWithoutChangingKeysAsync()
	{
		var path = Path.Combine(await Common.GetEmptyWorkDirAsync(), "software.json");
		var keys = NewKeys(path);
		keys.GenerateNewKey("synthetic-label", KeyState.Clean, false);
		keys.ToFile();
		var data = JObject.Parse(await File.ReadAllTextAsync(path));
		data["Icon"] = "Trezor";
		data["PreferPsbtWorkflow"] = true;
		data["SilentPaymentScanExtPubKey"] = keys.SegwitExtPubKey.ToString(Network.Main);
		data["SilentPaymentSpendExtPubKey"] = keys.TaprootExtPubKey!.ToString(Network.Main);
		await File.WriteAllTextAsync(path, data.ToString());
		var original = await File.ReadAllBytesAsync(path);
		var imported = KeyManager.FromFile(path);
		Assert.Equal(original, await File.ReadAllBytesAsync(path));
		Assert.Equal(keys.EncryptedSecret, imported.EncryptedSecret);
		Assert.Equal(keys.ChainCode, imported.ChainCode);
		Assert.Equal(keys.MasterFingerprint, imported.MasterFingerprint);
		Assert.Equal(keys.SegwitExtPubKey, imported.SegwitExtPubKey);
		Assert.Equal(keys.TaprootExtPubKey, imported.TaprootExtPubKey);
		Assert.Equal(keys.GetKeys().Select(k => (k.PubKey, k.Labels)), imported.GetKeys().Select(k => (k.PubKey, k.Labels)));
		imported.ToFile();
		var saved = JObject.Parse(await File.ReadAllTextAsync(path));
		Assert.Null(saved["Icon"]);
		Assert.Null(saved["PreferPsbtWorkflow"]);
		Assert.Null(saved["SilentPaymentScanExtPubKey"]);
		Assert.Null(saved["SilentPaymentSpendExtPubKey"]);
		Assert.Equal(keys.EncryptedSecret, KeyManager.FromFile(path).EncryptedSecret);
	}

	[Fact]
	public async Task RpcAndSchemeExposeOnlySoftwareWalletInformationAsync()
	{
		await using var app = new SingleWalletTests.SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		app.Session.Configure(app.NewKeys("secret"));
		var info = new MagicalCryptoWalletJsonRpcService(app.Global).WalletInfo();
		Assert.False(info.ContainsKey("isHardwareWallet"));
		Assert.False(info.ContainsKey("isWatchOnly"));
		Assert.NotNull(info["masterKeyFingerprint"]);
		var scheme = new Scheme(app.Global);
		foreach (var predicate in new[] { "wallet-hardware-wallet?", "wallet-watch-only?" })
		{
			Assert.NotNull(await Record.ExceptionAsync(() => scheme.ExecuteAsync($"({predicate} (wallet))")));
		}
	}

	[Fact]
	public void UnsignedPreviewNeedsNoPassphraseAndSigningStillRequiresAuthorization()
	{
		var keys = NewKeys();
		var income = Transaction.Create(Network.RegTest);
		income.Inputs.Add(new OutPoint(uint256.One, 0));
		income.Outputs.Add(Money.Coins(0.02m), keys.GetKeys()[0].GetAssumedScriptPubKey());
		var received = new SmartTransaction(income, new Height.ChainHeight(1));
		var coins = new[] { new SmartCoin(received, 0, keys.GetKeys()[0]) };
		var factory = new TransactionFactory(Network.RegTest, keys, new CoinsView(coins), new SyntheticTransactionStore(received), "wrong");
		using var recipient = new Key();
		var parameters = new TransactionParameters(new PaymentIntent(recipient, Money.Satoshis(5000)), new FeeRate(2m), true, false, null, false, false);
		var preview = factory.BuildTransaction(parameters);
		Assert.False(preview.Signed);
		Assert.False(preview.Psbt.IsAllFinalized());
		Assert.Throws<System.Security.SecurityException>(() => WalletAuthorization.Create(keys, "wrong"));
		using var authorization = WalletAuthorization.Create(keys, "secret");
		var signed = authorization.Sign(preview);
		Assert.True(signed.Signed);
		Assert.True(signed.Psbt.IsAllFinalized());
		Assert.True(Network.RegTest.CreateTransactionBuilder().AddCoins(coins.Select(c => c.Coin)).Verify(signed.Transaction.Transaction));
		Assert.False(preview.Psbt.IsAllFinalized());
	}

	private static KeyManager NewKeys(string? path = null) =>
		KeyManager.CreateNew(new Mnemonic(SingleWalletTests.SyntheticMnemonic), "secret", Network.RegTest, path);

	private sealed class SyntheticTransactionStore(SmartTransaction received) : ITransactionStore
	{
		public bool TryGetTransaction(uint256 hash, [NotNullWhen(true)] out SmartTransaction? transaction)
		{
			transaction = hash == received.GetHash() ? received : null;
			return transaction is not null;
		}
	}

	private static async Task WriteInvalidFileAsync(string path, string kind)
	{
		var keys = NewKeys(path);
		var data = JObject.Parse(await File.ReadAllTextAsync(path));
		switch (kind)
		{
			case "hardware": data["EncryptedSecret"] = null; data["ChainCode"] = null; break;
			case "watch-only": data["EncryptedSecret"] = null; data["ChainCode"] = null; data["MasterFingerprint"] = null; break;
			case "skeleton": data = new JObject { ["ExtPubKey"] = keys.SegwitExtPubKey.ToString(Network.RegTest), ["MasterFingerprint"] = keys.MasterFingerprint!.Value.ToString(), ["ColdCardFirmwareVersion"] = "2.1.0" }; break;
			case "missing-secret": data.Remove("EncryptedSecret"); break;
			case "missing-chain": data.Remove("ChainCode"); break;
			case "null-chain": data["ChainCode"] = null; break;
			case "short-chain": data["ChainCode"] = Convert.ToBase64String(new byte[31]); break;
			case "long-chain": data["ChainCode"] = Convert.ToBase64String(new byte[33]); break;
			case "invalid-chain": data["ChainCode"] = "not base64"; break;
			default: throw new ArgumentOutOfRangeException(nameof(kind));
		}
		await File.WriteAllTextAsync(path, data.ToString(), Encoding.UTF8);
	}

	private static Dictionary<string, byte[]> ReadFiles(string directory) => Directory.GetFiles(directory).ToDictionary(path => Path.GetFileName(path)!, File.ReadAllBytes);
	private static void AssertFilesUnchanged(string directory, Dictionary<string, byte[]> originals)
	{
		Assert.Equal(originals.Keys.Order(), Directory.GetFiles(directory).Select(Path.GetFileName).Order());
		foreach (var (name, bytes) in originals) { Assert.Equal(bytes, File.ReadAllBytes(Path.Combine(directory, name))); }
	}
	private static void AssertInvalidError(Exception? error, bool unsupported)
	{
		if (unsupported) { Assert.Contains("no local private keys", Assert.IsType<NotSupportedException>(error).Message); }
		else { Assert.Contains("32 bytes", Assert.IsType<InvalidDataException>(error).Message); }
	}
}
