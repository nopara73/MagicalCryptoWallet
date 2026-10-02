using System.Buffers.Binary;
using System.Text.Json;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.CompactFilters;

if (args.Length != 3) { return 2; }
using var watchdog = new Timer(static _ => Environment.Exit(78), null,
    TimeSpan.FromMinutes(2), Timeout.InfiniteTimeSpan);
if (args[0] == "unit")
{
    var checks = 0;
    var fake = new FakeService();
    using var binding = McwApplicationServices.Bind(fake);
    foreach (var response in new byte[][] { [], [2], [0, 0] })
    {
        fake.Response = response;
        await ExpectAsync<IOException>(() => McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], [])); checks++;
    }
    foreach (var response in new byte[][] { [0], [1] })
    {
        fake.Response = response;
        if (await McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], []) != (response[0] == 1)) { throw new Exception("Boolean response mismatch."); }
        checks++;
    }
    var before = fake.Calls;
    await ExpectAsync<ArgumentNullException>(() => McwCompactFilterMatcher.MatchAnyAsync(null!, new byte[32], [])); checks++;
    await ExpectAsync<ArgumentNullException>(() => McwCompactFilterMatcher.MatchAnyAsync([0], null!, [])); checks++;
    await ExpectAsync<ArgumentNullException>(() => McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], null!)); checks++;
    await ExpectAsync<ArgumentException>(() => McwCompactFilterMatcher.MatchAnyAsync([0], new byte[31], [])); checks++;
    await ExpectAsync<ArgumentException>(() => McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], [null!])); checks++;
    await ExpectAsync<IOException>(() => McwCompactFilterMatcher.MatchAnyAsync(new byte[1024 * 1024], new byte[32], [])); checks++;
    await ExpectAsync<IOException>(() => McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], new byte[65537][])); checks++;
    using var canceled = new CancellationTokenSource(); canceled.Cancel();
    await ExpectAsync<OperationCanceledException>(() => McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], [], canceled.Token)); checks++;
    if (fake.Calls != before) { throw new Exception("Locally invalid/canceled request reached the host."); }
    using var forwarded = new CancellationTokenSource();
    fake.Response = [0];
    await McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], [], forwarded.Token);
    if (fake.LastCancellation != forwarded.Token) { throw new Exception("Cancellation token was not forwarded."); }
    checks++;
    File.WriteAllText(args[2], JsonSerializer.Serialize(new { adapter_checks = checks, payload_order_verified = fake.PayloadVerified }));
    return 0;
}

using var host = ManagedApplicationHost.Connect();
using var stop = new CancellationTokenSource();
host.BindShutdown(stop.Cancel);
using var source = JsonDocument.Parse(File.ReadAllBytes(args[1]));
var typed = 0;
var errors = 0;
var valid = new List<JsonElement>();
foreach (var item in source.RootElement.GetProperty("cases").EnumerateArray())
{
    var encoded = Convert.FromHexString(item.GetProperty("filter").GetString()!);
    var hash = Convert.FromHexString(item.GetProperty("hash").GetString()!);
    var queries = item.GetProperty("queries").EnumerateArray().Select(x => Convert.FromHexString(x.GetString()!)).ToArray();
    if (item.GetProperty("error").GetBoolean())
    {
        await ExpectAsync<IOException>(() => McwCompactFilterMatcher.MatchAnyAsync(encoded, hash, queries)); errors++;
    }
    else
    {
        if (await McwCompactFilterMatcher.MatchAnyAsync(encoded, hash, queries) != item.GetProperty("expected").GetBoolean()) { throw new Exception("Host case mismatch: " + item.GetProperty("name").GetString()); }
        valid.Add(item);
    }
    typed++;
}
var malformedPayloads = 0;
foreach (var raw in source.RootElement.GetProperty("raw_errors").EnumerateArray())
{
    await ExpectAsync<IOException>(() => McwApplicationServices.Current.RequestAsync(McwCompactFilterMatcher.MatchAnyOperation, Convert.FromHexString(raw.GetString()!)));
    malformedPayloads++;
}
await Task.WhenAll(valid.Select(async item =>
{
    var answer = await McwCompactFilterMatcher.MatchAnyAsync(Convert.FromHexString(item.GetProperty("filter").GetString()!),
        Convert.FromHexString(item.GetProperty("hash").GetString()!),
        item.GetProperty("queries").EnumerateArray().Select(x => Convert.FromHexString(x.GetString()!)).ToArray());
    if (answer != item.GetProperty("expected").GetBoolean()) { throw new Exception("Concurrent host response mismatch."); }
}));
await ExpectAsync<IOException>(() => McwApplicationServices.Current.RequestAsync(ushort.MaxValue, ReadOnlyMemory<byte>.Empty));
using (var canceled = new CancellationTokenSource())
{
    canceled.Cancel();
    await ExpectAsync<OperationCanceledException>(() => McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], [], canceled.Token));
}
if (await McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], [])) { throw new Exception("Empty-filter recovery failed."); }
// Exercise accepted bounds through the actual framed transport, alongside local
// rejection of the adjacent oversized payload. These use synthetic empty filters.
var maximumQueries = Enumerable.Repeat(Array.Empty<byte>(), 65_536).ToArray();
if (await McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], maximumQueries)) { throw new Exception("Maximum-query empty filter mismatch."); }
var maximumScript = new byte[1024 * 1024 - 16 - 45];
if (await McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], [maximumScript])) { throw new Exception("Maximum-frame empty filter mismatch."); }
await ExpectAsync<IOException>(() => McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], [new byte[maximumScript.Length + 1]]));
if (await McwCompactFilterMatcher.MatchAnyAsync([0], new byte[32], [])) { throw new Exception("Boundary-error recovery failed."); }
File.WriteAllText(args[2], JsonSerializer.Serialize(new {
    typed_cases = typed, expected_codec_errors = errors, malformed_payloads = malformedPayloads,
    concurrent_cases = valid.Count, pre_cancellation_and_recovery = true, unknown_operation_recovery = true,
    maximum_query_and_frame_bounds = true, oversized_frame_local_rejection_and_recovery = true
}));
return 0;

static async Task ExpectAsync<T>(Func<Task> action) where T : Exception
{
    try { await action(); throw new Exception("Expected " + typeof(T).Name); }
    catch (T) { }
}

sealed class FakeService : IMcwApplicationServices
{
    public byte[] Response = [0];
    public int Calls;
    public bool PayloadVerified;
    public CancellationToken LastCancellation;
    public CancellationToken Stopped => CancellationToken.None;
    public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
    {
        Calls++;
        LastCancellation = cancellationToken;
        if (operation != 0x0702 || payload.Length != 41 || payload.Span[..32].ContainsAnyExcept((byte)0)
            || BinaryPrimitives.ReadUInt32LittleEndian(payload.Span[32..]) != 1 || payload.Span[36] != 0
            || BinaryPrimitives.ReadUInt32LittleEndian(payload.Span[37..]) != 0) { throw new Exception("Typed adapter payload mismatch."); }
        PayloadVerified = true;
        return Task.FromResult(Response);
    }
}
