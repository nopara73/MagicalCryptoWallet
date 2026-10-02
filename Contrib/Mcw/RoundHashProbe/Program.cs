using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.CoinJoin;
using MagicalCryptoWallet.WabiSabi.Client.RoundStateAwaiters;
using MagicalCryptoWallet.WabiSabi.Coordinator.PostRequests;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.WabiSabi.Crypto;
using MagicalCryptoWallet.WabiSabi.Models;
using MagicalCryptoWallet.WabiSabi.Models.MultipartyTransaction;
using WabiSabi.Crypto;
using WabiSabi.Crypto.Groups;

// Uses actual production types and the untouched coordinator reference. All
// fixtures are synthetic public round parameters, never wallets or credentials.
if (args.Length < 2) { return 2; }
var action = args[0];
var destination = Path.GetFullPath(args[1]);
var fixtures = CreateFixtures().ToArray();
if (action == "reference")
{
    var lines = new List<string> { "# Unchanged managed RoundHasher.CalculateHash; raw uint256.ToBytes(), synthetic public parameters." };
    lines.AddRange(fixtures.Select(x => string.Join('\t', x.Name, Convert.ToHexString(McwRoundHash.Encode(x.Round)).ToLowerInvariant(), Convert.ToHexString(Reference(x.Round).ToBytes()).ToLowerInvariant())));
    File.WriteAllLines(destination, lines, new UTF8Encoding(false));
    Console.Error.WriteLine($"Managed round reference vectors: {fixtures.Length}");
    return 0;
}
#if MCW_ROUND_HASH_ACTIVATED
if (action == "unavailable")
{
    Console.SetOut(Console.Error);
    await ExpectAsync<InvalidOperationException>(() => fixtures[0].Round.IsRoundIdMatchingAsync());
    using var canceled = new CancellationTokenSource(); canceled.Cancel();
    await ExpectAsync<OperationCanceledException>(() => fixtures[0].Round.IsRoundIdMatchingAsync(canceled.Token));
    using (McwApplicationServices.Bind(new InvalidResponse()))
    {
        await ExpectAsync<IOException>(() => fixtures[0].Round.IsRoundIdMatchingAsync());
    }
    var baseline = fixtures[0].Round;
    var parameters = baseline.CoinjoinState.Parameters;
    await ExpectAsync<InvalidDataException>(() => McwRoundHash.CalculateAsync(baseline with { CoinjoinState = new ConstructionState(parameters with { CoordinationIdentifier = new string('a', 65_537) }) }));
    await ExpectAsync<NotSupportedException>(() => McwRoundHash.CalculateAsync(baseline with { CoinjoinState = new ConstructionState(parameters with { AllowedInputTypes = ImmutableSortedSet.Create((ScriptType)(-1)) }) }));
    // No host is bound; poll failure leaves previously accepted rounds intact.
    await CheckUpdaterAsync(fixtures[0].Round, accepted: false);
    File.WriteAllText(destination, "{\"unavailable\":true,\"pre_canceled\":true,\"short_reply_rejected\":true,\"poll_state_preserved\":true,\"encoder_bounds\":true}");
    return 0;
}
if (action != "host") { return 2; }
using var host = ManagedApplicationHost.Connect();
using var stop = new CancellationTokenSource(); host.BindShutdown(stop.Cancel);
var vectors = File.ReadAllLines(args[2]).Where(x => !x.StartsWith('#')).Select(x => x.Split('\t')).ToArray();
Assert(vectors.Length == fixtures.Length, "Fixture count changed.");
for (var index = 0; index < fixtures.Length; index++)
{
    var (name, round) = fixtures[index];
    Assert(name == vectors[index][0], "Fixture order changed.");
    Assert(McwRoundHash.Encode(round).AsSpan().SequenceEqual(Convert.FromHexString(vectors[index][1])), "Managed wire bytes changed.");
    var calculated = await McwRoundHash.CalculateAsync(round, stop.Token);
    Assert(calculated.ToBytes().AsSpan().SequenceEqual(Convert.FromHexString(vectors[index][2])), "Rust differs from the managed reference.");
    Assert(await (round with { Id = calculated }).IsRoundIdMatchingAsync(stop.Token), "Matching round rejected.");
    Assert(!await (round with { Id = uint256.One }).IsRoundIdMatchingAsync(stop.Token), "Tampered round accepted.");
}
var valid = fixtures[0].Round with { Id = Reference(fixtures[0].Round) };
await Task.WhenAll(Enumerable.Range(0, 64).Select(async _ => Assert(await valid.IsRoundIdMatchingAsync(stop.Token), "Concurrent comparison failed.")));
// Prove the production updater accepts only verified new rounds, and preserves
// its old dictionary/awaiters on a mismatch. Existing rounds are not re-hashed.
await CheckUpdaterAsync(valid, accepted: true);
await CheckUpdaterAsync(valid with { InputRegistrationTimeout = valid.InputRegistrationTimeout + TimeSpan.FromTicks(1) }, accepted: false);
using (var cancellation = new CancellationTokenSource())
{
    // Queue expensive QR work on the same host, so cancellation occurs after
    // the round request is written and before the pure result is returned.
    var queue = Enumerable.Range(0, 12).Select(_ => host.GenerateQrAsync(new string('a', 2331))).ToArray();
    var pending = valid.IsRoundIdMatchingAsync(cancellation.Token);
    await cancellation.CancelAsync();
    await ExpectAsync<OperationCanceledException>(() => pending);
    await Task.WhenAll(queue);
    Assert(await valid.IsRoundIdMatchingAsync(), "Late reply broke the next request.");
}
using (var cancellation = new CancellationTokenSource())
using (var awaiter = new RoundStateAwaiter(_ => false, null, null, cancellation.Token))
{
    var queue = Enumerable.Range(0, 12).Select(_ => host.GenerateQrAsync(new string('a', 2331))).ToArray();
    var state = new RoundsState(DateTime.MinValue, TimeSpan.Zero, [], [awaiter]);
    var update = RoundStateUpdater.Create(new StatusOnly(valid))(new RoundUpdateMessage.UpdateMessage(DateTime.UtcNow), state, cancellation.Token);
    await cancellation.CancelAsync();
    await ExpectAsync<OperationCanceledException>(() => update);
    Assert(state.Rounds.Count == 0, "Cancelled update accepted new state.");
    await Task.WhenAll(queue);
    Assert(await valid.IsRoundIdMatchingAsync(), "Cancelled poll poisoned the host.");
}
var payload = McwRoundHash.Encode(valid);
for (var end = 0; end < payload.Length; end++)
{
    await ExpectAsync<IOException>(() => McwApplicationServices.Current.RequestAsync(McwRoundHash.Operation, payload.AsMemory(0, end)));
}
foreach (var offset in new[] { 0, 2 })
{
    var bad = (byte[])payload.Clone(); bad[offset] = 2;
    await ExpectAsync<IOException>(() => McwApplicationServices.Current.RequestAsync(McwRoundHash.Operation, bad));
}
await ExpectAsync<IOException>(() => McwApplicationServices.Current.RequestAsync(McwRoundHash.Operation, payload.Concat(new byte[] { 0 }).ToArray()));
Assert(await valid.IsRoundIdMatchingAsync(), "Malformed request poisoned the host.");
File.WriteAllText(destination, JsonSerializer.Serialize(new { vectors = fixtures.Length, comparisons = fixtures.Length * 2 + 64, updaterAcceptance = true,
    updaterMismatch = true, cancellation = true, updaterCancellation = true, lateReply = true, truncated = payload.Length, malformedIsolation = true, wallets = 0, network = false }));
return 0;

static async Task CheckUpdaterAsync(RoundState round, bool accepted)
{
    using var awaiter = new RoundStateAwaiter(_ => false, null, null, CancellationToken.None);
    var previous = round with { Id = uint256.One };
    var state = new RoundsState(DateTime.MinValue, TimeSpan.Zero, new Dictionary<uint256, RoundState> { [previous.Id] = previous }, [awaiter]);
    var handler = RoundStateUpdater.Create(new StatusOnly(round));
    var result = await handler(new RoundUpdateMessage.UpdateMessage(DateTime.UtcNow), state, CancellationToken.None);
    Assert(result.Rounds.ContainsKey(round.Id) == accepted, "Updater verification gate failed.");
    Assert((result.ConsecutiveFailures == 0) == accepted, "Updater failure accounting changed.");
    if (!accepted) { Assert(ReferenceEquals(result.Rounds, state.Rounds) && result.Awaiters.Contains(awaiter), "Failure mutated accepted state."); }
}

static async Task ExpectAsync<T>(Func<Task> action) where T : Exception
{
    try { await action(); }
    catch (T) { return; }
    throw new Exception($"Expected {typeof(T).Name}.");
}
static void Assert(bool condition, string message) { if (!condition) { throw new Exception(message); } }
#else
Console.Error.WriteLine("Round hash caller and host registration have not been activated together. Build the probe with RoundHashActivated=true after integration.");
return 2;
#endif

static uint256 Reference(RoundState r)
{
    var p = r.CoinjoinState.Parameters;
    return RoundHasher.CalculateHash(r.InputRegistrationStart, r.InputRegistrationTimeout, p.ConnectionConfirmationTimeout, p.OutputRegistrationTimeout,
        p.TransactionSigningTimeout, p.AllowedInputAmounts, p.AllowedInputTypes, p.AllowedOutputAmounts, p.AllowedOutputTypes, p.Network,
        p.MiningFeeRate.FeePerK, p.MaxTransactionSize, p.MinRelayTxFee.FeePerK, p.MaxAmountCredentialValue, p.MaxVsizeCredentialValue,
        p.MaxVsizeAllocationPerAlice, p.MaxSuggestedAmount, p.CoordinationIdentifier, r.AmountCredentialIssuerParameters, r.VsizeCredentialIssuerParameters);
}

static IEnumerable<(string Name, RoundState Round)> CreateFixtures()
{
    var p = new RoundParameters(Network.Main, new FeeRate(Money.Satoshis(1234)), Money.Satoshis(123_456_789), 2, 10,
        new MoneyRange(Money.Satoshis(5000), Money.Satoshis(4_300_000_000)), new MoneyRange(Money.Satoshis(3000), Money.Satoshis(4_200_000_000)),
        ImmutableSortedSet.Create(ScriptType.P2WPKH, ScriptType.Taproot), ImmutableSortedSet.Create(ScriptType.P2WPKH, ScriptType.Taproot),
        TimeSpan.FromTicks(600_000_001), TimeSpan.FromTicks(200_000_003), TimeSpan.FromTicks(300_000_005), TimeSpan.FromTicks(400_000_007),
        TimeSpan.FromTicks(500_000_009), "Synthetic Round Hash / 公開 🦀", false);
    var r = new RoundState(uint256.Zero, uint256.Zero, new CredentialIssuerParameters(Generators.G, Generators.Gw),
        new CredentialIssuerParameters(Generators.Gh, Generators.Gg), Phase.InputRegistration, EndRoundState.None,
        DateTimeOffset.FromUnixTimeMilliseconds(1_790_000_000_123), p.StandardInputRegistrationTimeout, new ConstructionState(p));
    yield return ("baseline", r);
    foreach (var network in new[] { Network.TestNet, Network.RegTest }) { yield return (network.ToString(), Replace(p with { Network = network })); }
    yield return ("pre-epoch", r with { InputRegistrationStart = DateTimeOffset.FromUnixTimeMilliseconds(-123) });
    yield return ("same-timezone-instant", r with { InputRegistrationStart = r.InputRegistrationStart.ToOffset(TimeSpan.FromHours(8)) });
    yield return ("same-millisecond", r with { InputRegistrationStart = r.InputRegistrationStart.AddTicks(1) });
    yield return ("timestamp-millisecond", r with { InputRegistrationStart = r.InputRegistrationStart.AddMilliseconds(1) });
    yield return ("input-timeout-tick", r with { InputRegistrationTimeout = r.InputRegistrationTimeout + TimeSpan.FromTicks(1) });
    yield return ("input-timeout-min-i64", r with { InputRegistrationTimeout = TimeSpan.MinValue });
    yield return ("input-timeout-max-i64", r with { InputRegistrationTimeout = TimeSpan.MaxValue });
    yield return ("confirmation-tick", Replace(p with { ConnectionConfirmationTimeout = p.ConnectionConfirmationTimeout + TimeSpan.FromTicks(1) }));
    yield return ("output-tick", Replace(p with { OutputRegistrationTimeout = p.OutputRegistrationTimeout + TimeSpan.FromTicks(1) }));
    yield return ("signing-tick", Replace(p with { TransactionSigningTimeout = p.TransactionSigningTimeout + TimeSpan.FromTicks(1) }));
    yield return ("input-min-sat", Replace(p with { AllowedInputAmounts = p.AllowedInputAmounts with { Min = p.AllowedInputAmounts.Min + Money.Satoshis(1) } }));
    yield return ("input-max-sat", Replace(p with { AllowedInputAmounts = p.AllowedInputAmounts with { Max = p.AllowedInputAmounts.Max + Money.Satoshis(1) } }));
    yield return ("output-min-sat", Replace(p with { AllowedOutputAmounts = p.AllowedOutputAmounts with { Min = p.AllowedOutputAmounts.Min + Money.Satoshis(1) } }));
    yield return ("output-max-sat", Replace(p with { AllowedOutputAmounts = p.AllowedOutputAmounts with { Max = p.AllowedOutputAmounts.Max + Money.Satoshis(1) } }));
    yield return ("input-types-empty", Replace(p with { AllowedInputTypes = ImmutableSortedSet<ScriptType>.Empty }));
    yield return ("output-types-all", Replace(p with { AllowedOutputTypes = Enum.GetValues<ScriptType>().ToImmutableSortedSet() }));
    yield return ("input-types-reversed", Replace(p with { AllowedInputTypes = p.AllowedInputTypes.WithComparer(Comparer<ScriptType>.Create((a, b) => b.CompareTo(a))) }));
    yield return ("fee-per-k-sat", Replace(p with { MiningFeeRate = new FeeRate(Money.Satoshis(1235)) }));
    yield return ("relay-per-k-sat", Replace(p with { MinRelayTxFee = new FeeRate(Money.Satoshis(p.MinRelayTxFee.FeePerK.Satoshi + 1)) }));
    yield return ("allocation-integer", Replace(p with { MaxVsizeAllocationPerAlice = p.MaxVsizeAllocationPerAlice + 1 }));
    yield return ("suggested-sat", Replace(p with { MaxSuggestedAmount = p.MaxSuggestedAmount + Money.Satoshis(1) }));
    foreach (var value in new[] { "", "MixedCASE \0\r\n", "\uFEFFexplicit BOM", "unpaired \ud800 replacement" }) { yield return ($"identifier-{Encoding.UTF8.GetByteCount(value)}", Replace(p with { CoordinationIdentifier = value })); }
    foreach (var length in new[] { 164, 165, 166, 167, 332, 65_536 }) { yield return ($"identifier-{length}", Replace(p with { CoordinationIdentifier = new string('a', length) })); }
    yield return ("amount-cw", r with { AmountCredentialIssuerParameters = new(Generators.Gx0, Generators.Gw) });
    yield return ("amount-i", r with { AmountCredentialIssuerParameters = new(Generators.G, Generators.Gx1) });
    yield return ("vsize-cw", r with { VsizeCredentialIssuerParameters = new(Generators.GV, Generators.Gg) });
    yield return ("vsize-i", r with { VsizeCredentialIssuerParameters = new(Generators.Gh, Generators.Gs) });
    RoundState Replace(RoundParameters parameters) => r with { CoinjoinState = new ConstructionState(parameters) };
}

#if MCW_ROUND_HASH_ACTIVATED
sealed class InvalidResponse : IMcwApplicationServices
{
    public CancellationToken Stopped => CancellationToken.None;
    public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default) => Task.FromResult(new byte[31]);
}

sealed class StatusOnly(RoundState round) : IWabiSabiApiRequestHandler
{
    public Task<RoundStateResponse> GetStatusAsync(RoundStateRequest request, CancellationToken cancellationToken) => Task.FromResult(new RoundStateResponse([round]));
    public Task<InputRegistrationResponse> RegisterInputAsync(InputRegistrationRequest r, CancellationToken c) => throw new NotSupportedException();
    public Task<ConnectionConfirmationResponse> ConfirmConnectionAsync(ConnectionConfirmationRequest r, CancellationToken c) => throw new NotSupportedException();
    public Task RegisterOutputAsync(OutputRegistrationRequest r, CancellationToken c) => throw new NotSupportedException();
    public Task RemoveInputAsync(InputsRemovalRequest r, CancellationToken c) => throw new NotSupportedException();
    public Task SignTransactionAsync(TransactionSignaturesRequest r, CancellationToken c) => throw new NotSupportedException();
    public Task<ReissueCredentialResponse> ReissuanceAsync(ReissueCredentialRequest r, CancellationToken c) => throw new NotSupportedException();
    public Task ReadyToSignAsync(ReadyToSignRequestRequest r, CancellationToken c) => throw new NotSupportedException();
}
#endif
