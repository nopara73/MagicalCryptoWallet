// Test-only child of the actual mcw host. No wallet, network or user cache is opened.
using System;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Blocks;
using MagicalCryptoWallet.Tests.UnitTests.Wallet;
using MagicalCryptoWallet.Wallets;

if (args.Length != 4 || Environment.GetEnvironmentVariable("MCW_HOSTED") != "1") { return 2; }
var action = args[0];
var report = Path.GetFullPath(args[1]);
var vectors = Path.GetFullPath(args[2]);
var cache = Path.GetFullPath(args[3]);
using var host = ManagedApplicationHost.Connect();
var services = McwApplicationServices.Current;
var headerService = new McwBlockHeaderService();
var block = Network.Main.Consensus.ConsensusFactory.CreateBlock();
block.Header.Version = 4;
block.Header.Nonce = 0x80000001;
var expectedHash = block.GetHash(); // Transitional reference, never production fallback.
var path = Path.Combine(cache, expectedHash.ToString());
var repository = new FileSystemBlockRepository(cache, Network.Main);

if (action == "recover")
{
    Check(File.Exists(path), "Host failure removed the cache before recovery.");
    Check((await repository.TryGetBlockAsync(expectedHash, CancellationToken.None)) is not null, "Cache could not be retried through a fresh real host.");
    File.WriteAllText(report, JsonSerializer.Serialize(new { recovered = true, bytesUnchanged = File.ReadAllBytes(path).SequenceEqual(block.ToBytes()) }));
    return 0;
}
if (action != "verify") { return 2; }

var unitCases = 0;
var unitTests = new FileSystemBlockRepositoryTests();
foreach (var method in typeof(FileSystemBlockRepositoryTests).GetMethods(BindingFlags.Instance | BindingFlags.Public)
    .Where(method => method.Name.EndsWith("Async", StringComparison.Ordinal)))
{
    await (Task)method.Invoke(unitTests, null)!;
    unitCases++;
}

var hashes = 0;
foreach (var line in File.ReadLines(vectors).Where(line => !line.StartsWith('#')))
{
    var fields = line.Split('\t');
    var header = Convert.FromHexString(fields[1]);
    var native = await headerService.HashAsync(header, CancellationToken.None);
    var baseline = BlockHeader.Parse(fields[1], Network.Main);
    Check(baseline.ToBytes().SequenceEqual(header), "NBitcoin header serialization changed the synthetic input.");
    Check(native.ToBytes().SequenceEqual(Convert.FromHexString(fields[2])), "Native digest differs from independent Core/hashlib.");
    Check(native == baseline.GetHash() && native.ToString() == fields[3], "Native/NBitcoin filename or byte order differs.");
    hashes++;
}
Check(hashes == 256, "Synthetic vector coverage is incomplete.");

var rejected = 0;
foreach (var length in Enumerable.Range(0, 80).Concat([81, 1_048_560]))
{
    await ExpectAsync<IOException>(() => services.RequestAsync(McwBlockHeaderService.HashHeaderOperation, new byte[length]));
    rejected++;
}
await ExpectAsync<IOException>(() => services.RequestAsync(0x0E01, new byte[80]));
Check(await headerService.HashAsync(block.Header.ToBytes(), CancellationToken.None) == expectedHash, "Host did not recover after malformed requests.");

Directory.CreateDirectory(cache);
// Existing bytes are consumed without rewrite or any storage-format change.
await File.WriteAllBytesAsync(path, block.ToBytes());
Check((await repository.TryGetBlockAsync(expectedHash, CancellationToken.None))?.ToBytes().SequenceEqual(block.ToBytes()) == true, "Existing cache bytes did not round trip.");
var wrongHash = new uint256(1);
var wrongPath = Path.Combine(cache, wrongHash.ToString());
await File.WriteAllBytesAsync(wrongPath, block.ToBytes());
Check(await repository.TryGetBlockAsync(wrongHash, CancellationToken.None) is null && !File.Exists(wrongPath), "Wrong-identity cache was accepted.");
var truncatedHash = new uint256(2);
var truncatedPath = Path.Combine(cache, truncatedHash.ToString());
await File.WriteAllBytesAsync(truncatedPath, new byte[79]);
Check(await repository.TryGetBlockAsync(truncatedHash, CancellationToken.None) is null && !File.Exists(truncatedPath), "Truncated cache was accepted.");
var malformed = new byte[81];
block.Header.ToBytes().CopyTo(malformed, 0);
malformed[80] = 1;
await File.WriteAllBytesAsync(path, malformed);
Check(await repository.TryGetBlockAsync(expectedHash, CancellationToken.None) is null && !File.Exists(path), "Retained block mapping failed to reject a corrupt body.");
await repository.SaveAsync(block, CancellationToken.None);
Check(File.ReadAllBytes(path).SequenceEqual(block.ToBytes()), "Native filename/write bytes differ from the retained format.");
await repository.SaveAsync(block, CancellationToken.None);
Check((await repository.TryGetBlockAsync(expectedHash, CancellationToken.None)) is not null, "Redownload/retry failed.");

using (var canceled = new CancellationTokenSource())
{
    canceled.Cancel();
    await ExpectAsync<OperationCanceledException>(() => repository.TryGetBlockAsync(expectedHash, canceled.Token));
    Check(File.ReadAllBytes(path).SequenceEqual(block.ToBytes()), "Cancellation modified a valid cache.");
    Check((await repository.TryGetBlockAsync(expectedHash, CancellationToken.None)) is not null, "Retry after cancellation failed.");
}
var pruneCache = Path.Combine(cache, "prune");
Directory.CreateDirectory(pruneCache);
var oldPath = Path.Combine(pruneCache, "old-block");
await File.WriteAllBytesAsync(oldPath, new byte[1024 * 1024]);
File.SetLastAccessTimeUtc(oldPath, DateTime.UtcNow.AddDays(-1));
await new FileSystemBlockRepository(pruneCache, Network.Main, 1).SaveAsync(block, CancellationToken.None);
Check(!File.Exists(oldPath) && File.Exists(Path.Combine(pruneCache, expectedHash.ToString())), "Pruning contract changed.");

// Actual binding teardown, not a substitute hashing implementation. The next
// child invocation must retry these exact preserved bytes through a fresh host.
host.Dispose();
await ExpectAsync<McwBlockHeaderServiceException>(() => repository.TryGetBlockAsync(expectedHash, CancellationToken.None));
Check(File.ReadAllBytes(path).SequenceEqual(block.ToBytes()), "Actual host teardown deleted or modified the cache.");
File.WriteAllText(report, JsonSerializer.Serialize(new { syntheticHeaders = hashes, nBitcoinCoreHashComparisons = hashes, rejectedNativePayloads = rejected, faultInjectionTests = unitCases, realCacheCases = 9, actualHostTeardownPreservesCache = true }));
return 0;

static void Check(bool condition, string message)
{
    if (!condition) { throw new InvalidOperationException(message); }
}
static async Task ExpectAsync<T>(Func<Task> action) where T : Exception
{
    try { await action(); }
    catch (T) { return; }
    throw new InvalidOperationException("Expected " + typeof(T).Name + ".");
}
