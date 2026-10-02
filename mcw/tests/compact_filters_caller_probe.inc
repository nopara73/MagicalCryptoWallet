using System.Buffers.Binary;
using System.Diagnostics;
using System.Reflection;
using System.Security.Cryptography;
using System.Text.Json;
using MagicalCryptoWallet.Backend.Models;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.Blocks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.Mempool;
using MagicalCryptoWallet.Blockchain.TransactionProcessing;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Stores;
using MagicalCryptoWallet.Tests.UnitTests;
using MagicalCryptoWallet.Tests.UnitTests.Services;
using MagicalCryptoWallet.Tests.UnitTests.Wallet.FilterProcessor;
using MagicalCryptoWallet.Wallets;
using NBitcoin;
using Xunit;

if (args.Length != 2) { return 2; }
var reportPath = Path.GetFullPath(args[0]);
var syntheticRoot = Path.GetFullPath(args[1]);
Directory.CreateDirectory(syntheticRoot);
// Keep retained fixture state inside this synthetic child on every desktop OS.
// Production EnvironmentHelpers and the user's home directory stay unchanged.
MagicalCryptoWallet.Tests.Helpers.Common.UseSyntheticDataRoot(syntheticRoot);
MagicalCryptoWallet.Logging.Logger.Configure(filePath: Path.Combine(syntheticRoot, "caller.log"),
    logModes: [MagicalCryptoWallet.Logging.LogMode.File]);
using var watchdog = new Timer(static _ => Environment.Exit(78), null,
    TimeSpan.FromMinutes(3), Timeout.InfiniteTimeSpan);
using var host = ManagedApplicationHost.Connect();
var results = new List<object>();
foreach (var name in new[] { "matched", "not-matched", "no-keys", "malformed",
    "frame-limit", "missing-block", "download-canceled" })
{
    results.Add(await RunCallerCaseAsync(name, host, syntheticRoot));
}
foreach (var name in new[] { "pre-canceled", "custom-p", "custom-m" })
{
    results.Add(await DirectCallerGuardAsync(name, host, syntheticRoot));
}

// Execute retained test methods from unchanged source, using real xUnit asserts.
// These selected methods use synthetic providers/state and perform no network I/O.
Assert.StartsWith(syntheticRoot + Path.DirectorySeparatorChar,
    Path.GetFullPath(MagicalCryptoWallet.Tests.Helpers.Common.DataDir), StringComparison.OrdinalIgnoreCase);
var retained = new List<string>();
await new SingleWalletTests().CancellationWhileWaitingForHeadersStopsCleanlyAsync(false);
retained.Add("SingleWalletTests.CancellationWhileWaitingForHeadersStopsCleanlyAsync(false)");
await new SingleWalletTests().CancellationWhileWaitingForHeadersStopsCleanlyAsync(true);
retained.Add("SingleWalletTests.CancellationWhileWaitingForHeadersStopsCleanlyAsync(true)");
await new BlockFilterIteratorTests().GetAndRemoveTestAsync();
retained.Add("BlockFilterIteratorTests.GetAndRemoveTestAsync");
new CompactFilterBlockHashReproTests().ValidFilterBytesAreAcceptedWithAnAttackerChosenBlockHash();
retained.Add("CompactFilterBlockHashReproTests.ValidFilterBytesAreAcceptedWithAnAttackerChosenBlockHash");

Assert.Same(McwApplicationServices.Current, host);
var core = typeof(WalletFilterProcessor).Assembly;
File.WriteAllText(reportPath, JsonSerializer.Serialize(new {
    caller_type = typeof(WalletFilterProcessor).FullName,
    caller_assembly = core.Location,
    caller_assembly_sha256 = Convert.ToHexString(SHA256.HashData(File.ReadAllBytes(core.Location))).ToLowerInvariant(),
    caller_cases = results, retained_tests = retained,
    synthetic_state_root = syntheticRoot, current_host_binding_restored = true,
    production_wallet_or_network_used = false, legacy_matching_fallback = false
}, new JsonSerializerOptions { WriteIndented = true }));
return 0;

static KeyManager NewKeys(bool indexed)
{
    var keys = KeyManager.CreateNew(new Mnemonic(SingleWalletTests.SyntheticMnemonic), "", Network.RegTest);
    if (indexed) { _ = keys.GetNextReceiveKey(LabelsArray.Empty, ScriptPubKeyType.Segwit); }
    Assert.Null(keys.FilePath);
    Assert.Equal(indexed, keys.UnsafeGetSynchronizationInfos().Length != 0);
    return keys;
}

static async Task<object> RunCallerCaseAsync(string name, ManagedApplicationHost host, string root)
{
    var keys = NewKeys(name != "no-keys");
    var events = new EventBus();
    var headers = new FilterHeaderChain();
    using var transactions = new AllTransactionStore(SqliteStorageHelper.InMemoryDatabase, Network.RegTest);
    await transactions.InitializeAsync();
    using var filters = new FilterStore(Path.Combine(root, name), Network.RegTest, headers, events);
    await filters.InitializeAsync(new MagicalCryptoWallet.Models.Height.ChainHeight(0), CancellationToken.None);
    var transactionProcessor = new TransactionProcessor(transactions, new MempoolService(events), keys, Money.Satoshis(100), events);
    var block = Network.RegTest.Consensus.ConsensusFactory.CreateBlock();
    block.Header.HashPrevBlock = Network.RegTest.GetGenesis().GetHash();
    block.Header.BlockTime = DateTimeOffset.FromUnixTimeSeconds(1_700_000_001);
    block.Header.Nonce = 73;
    var payment = Network.RegTest.CreateTransaction();
    payment.Inputs.Add(TxIn.CreateCoinbase(1));
    var matchingScript = keys.UnsafeGetSynchronizationInfos().FirstOrDefault() ?? new byte[] { 81 };
    payment.Outputs.Add(Money.Coins(1), new Script(matchingScript));
    block.Transactions.Add(payment);
    block.UpdateMerkleRoot();
    var hash = block.GetHash();
    var encoded = name is "not-matched" or "no-keys" ? new byte[] { 0 } :
        new GolombRiceFilterBuilder().SetKey(hash).AddEntries([matchingScript]).Build().ToBytes();
    if (name == "malformed") { encoded = [0, 0]; }
    if (name == "frame-limit") { encoded = new byte[1024 * 1024]; }
    var model = new FilterModel(new SmartHeader(hash, uint256.One, 1, block.Header.BlockTime), new GolombRiceFilter(encoded));
    await filters.AddNewFiltersAsync([model]);
    var downloads = 0;
    var downloadedEvents = 0;
    var processedEvents = 0;
    var relevantEvents = 0;
    var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    CancellationToken providerToken = default;
    using var downloaded = events.Subscribe<BlockDownloaded>(_ => Interlocked.Increment(ref downloadedEvents));
    using var processed = events.Subscribe<FilterProcessed>(_ => Interlocked.Increment(ref processedEvents));
    using var relevant = events.Subscribe<WalletRelevantTransactionProcessed>(_ => Interlocked.Increment(ref relevantEvents));
    BlockProvider provider = async (requested, token) => {
        Interlocked.Increment(ref downloads);
        if (requested != hash) { throw new InvalidOperationException("Caller requested a different synthetic block."); }
        providerToken = token;
        entered.TrySetResult();
        if (name == "download-canceled") { await Task.Delay(Timeout.InfiniteTimeSpan, token); }
        return name == "missing-block" ? null : block;
    };
    var trace = new TraceService(host, hash.ToBytes(), keys.UnsafeGetSynchronizationInfos());
    using var binding = new ObservedBinding(host, trace);
    using var caller = new WalletFilterProcessor(keys, transactions, filters, headers, transactionProcessor, provider, events);
    await caller.StartAsync(CancellationToken.None);
    if (name == "download-canceled") { await entered.Task.WaitAsync(TimeSpan.FromSeconds(15)); }
    else if (name is "malformed" or "frame-limit" or "missing-block")
    {
        await WaitForAsync(() => caller.WaitingForBlock, caller);
        Assert.Equal(0u, (uint)keys.GetBestHeight());
        Assert.False(caller.InitialSynchronizationFinished.IsCompleted);
    }
    else { await caller.InitialSynchronizationFinished.WaitAsync(TimeSpan.FromSeconds(15)); }
    await caller.StopAsync(CancellationToken.None).WaitAsync(TimeSpan.FromSeconds(15));
    Assert.True(caller.ExecuteTask!.IsCompleted);
    var success = name is "matched" or "not-matched" or "no-keys";
    Assert.Equal(success ? 1u : 0u, (uint)keys.GetBestHeight());
    Assert.Equal(success ? 1 : 0, processedEvents);
    Assert.Equal(name == "matched" ? 1 : 0, downloadedEvents);
    Assert.Equal(name == "matched" ? 1 : 0, relevantEvents);
    Assert.Equal(name == "matched" ? Money.Coins(1) : Money.Zero, transactionProcessor.Coins.TotalAmount());
    Assert.Equal(name is "matched" or "missing-block" or "download-canceled" ? 1 : 0, downloads);
    Assert.Equal(name is "no-keys" or "frame-limit" ? 0 : 1, trace.Calls);
    if (trace.Calls != 0) { Assert.True(trace.CallerStackObserved); Assert.True(trace.PayloadVerified); }
    if (name == "download-canceled") { Assert.True(providerToken.IsCancellationRequested); }
    Assert.Null(keys.FilePath);
    return new { name, host_requests = trace.Calls, trace.CallerStackObserved, trace.PayloadVerified,
        block_requests = downloads, block_downloaded_events = downloadedEvents, filter_processed_events = processedEvents,
        relevant_transaction_events = relevantEvents, final_height = (uint)keys.GetBestHeight(),
        execution_retired = caller.ExecuteTask.IsCompleted,
        cancellation_forwarded = name == "download-canceled" && providerToken.IsCancellationRequested };
}

static async Task<object> DirectCallerGuardAsync(string name, ManagedApplicationHost host, string root)
{
    var keys = NewKeys(true);
    var events = new EventBus(); var headers = new FilterHeaderChain();
    using var transactions = new AllTransactionStore(SqliteStorageHelper.InMemoryDatabase, Network.RegTest);
    using var filters = new FilterStore(Path.Combine(root, name), Network.RegTest, headers, events);
    var processor = new TransactionProcessor(transactions, null, keys, Money.Satoshis(100), events);
    var downloads = 0;
    BlockProvider provider = (_, _) => { downloads++; throw new InvalidOperationException("Pre-canceled caller reached block provider."); };
    using var caller = new WalletFilterProcessor(keys, transactions, filters, headers, processor, provider, events);
    var trace = new TraceService(host, new byte[32], keys.UnsafeGetSynchronizationInfos());
    using var binding = new ObservedBinding(host, trace);
    using var canceled = new CancellationTokenSource();
    if (name == "pre-canceled") { canceled.Cancel(); }
    var method = typeof(WalletFilterProcessor).GetMethod("ProcessFilterModelAsync", BindingFlags.Instance | BindingFlags.NonPublic)!;
    // P/M are not serialized in the retained filter store. Exercise custom
    // parameters on the actual selected method before any store roundtrip.
    var encoded = new byte[] { 0 };
    var gcs = name == "custom-p" ? new GolombRiceFilter(encoded, 20, 784_931) :
        name == "custom-m" ? new GolombRiceFilter(encoded, 19, 1 << 20) : new GolombRiceFilter(encoded);
    var filter = new FilterModel(new SmartHeader(uint256.Zero, uint256.Zero, 1, 0), gcs);
    if (name == "pre-canceled") {
        await Assert.ThrowsAnyAsync<OperationCanceledException>(() => (Task<bool>)method.Invoke(caller, [filter, canceled.Token])!);
    } else {
        await Assert.ThrowsAsync<InvalidOperationException>(() => (Task<bool>)method.Invoke(caller, [filter, canceled.Token])!);
    }
    Assert.Equal(0, trace.Calls); Assert.Equal(0, downloads); Assert.Equal(0u, (uint)keys.GetBestHeight());
    return new { name, host_requests = trace.Calls, block_requests = downloads, final_height = 0,
        actual_private_matching_method_invoked = true, cancellation_forwarded = name == "pre-canceled",
        parameters_rejected = name != "pre-canceled", p = gcs.P, m = gcs.M };
}

static async Task WaitForAsync(Func<bool> predicate, WalletFilterProcessor caller)
{
    using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(15));
    while (!predicate()) {
        if (caller.ExecuteTask is { IsFaulted: true } failed) { await failed; }
        await Task.Delay(10, timeout.Token);
    }
}

sealed class TraceService(IMcwApplicationServices inner, byte[] hash, byte[][] queries) : IMcwApplicationServices
{
    public int Calls;
    public bool CallerStackObserved;
    public bool PayloadVerified;
    public CancellationToken Stopped => inner.Stopped;
    public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
    {
        Interlocked.Increment(ref Calls);
        if (operation != 0x0702) { throw new IOException("Actual caller requested an unexpected operation."); }
        CallerStackObserved |= new StackTrace().GetFrames().Any(frame =>
            frame.GetMethod()?.DeclaringType?.FullName?.Contains("WalletFilterProcessor+<ProcessFilterModelAsync>", StringComparison.Ordinal) == true);
        var remaining = payload.Span;
        Assert.Equal(hash, remaining[..32].ToArray()); remaining = remaining[32..];
        var length = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(remaining)); remaining = remaining[(4 + length)..];
        Assert.Equal((uint)queries.Length, BinaryPrimitives.ReadUInt32LittleEndian(remaining)); remaining = remaining[4..];
        foreach (var query in queries) {
            var size = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(remaining));
            Assert.Equal(query, remaining.Slice(4, size).ToArray()); remaining = remaining[(4 + size)..];
        }
        Assert.True(remaining.IsEmpty); PayloadVerified = true;
        return inner.RequestAsync(operation, payload, cancellationToken);
    }
}

// The actual host allows exactly one registration. This test-only observer
// releases that registration before installing a pass-through, then restores
// the host's registration before its normal disposal. No production code or
// transport result is replaced, and a simultaneous second bind is rejected.
sealed class ObservedBinding : IDisposable
{
    private readonly ManagedApplicationHost _host;
    private readonly FieldInfo _registration;
    private readonly IDisposable _observed;
    public ObservedBinding(ManagedApplicationHost host, IMcwApplicationServices observer)
    {
        _host = host;
        _registration = typeof(ManagedApplicationHost).GetField("_serviceBinding", BindingFlags.Instance | BindingFlags.NonPublic)!;
        Assert.Same(host, McwApplicationServices.Current);
        Assert.Throws<InvalidOperationException>(() => McwApplicationServices.Bind(observer));
        ((IDisposable)_registration.GetValue(host)!).Dispose();
        _registration.SetValue(host, null);
        _observed = McwApplicationServices.Bind(observer);
        Assert.Same(observer, McwApplicationServices.Current);
        Assert.Throws<InvalidOperationException>(() => McwApplicationServices.Bind(host));
    }
    public void Dispose()
    {
        _observed.Dispose();
        _registration.SetValue(_host, McwApplicationServices.Bind(_host));
        Assert.Same(_host, McwApplicationServices.Current);
    }
}
