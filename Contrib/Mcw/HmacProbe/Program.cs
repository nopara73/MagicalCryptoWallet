using System;
using System.IO;
using System.Linq;
using System.Reflection;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Crypto;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Crypto;
using NBitcoin;

// Synthetic test child only. Actual domain classes, adapter and host source are
// compiled; the package never contains this probe. Reference facts are offline.
if (args.Length != 2) { return 2; }
var fixture = Path.GetFullPath(args[0]);
var report = Path.GetFullPath(args[1]);
var faultChecks = AdapterFailureChecks();
var callerBoundaryChecks = CallerBoundaryChecks();
var transportChecks = await TransportFaults.RunAsync();
using var host = ManagedApplicationHost.Connect();
var calls = 0;
foreach (var row in File.ReadLines(fixture))
{
	var fields = row.Split('\t');
	var operation = Convert.ToUInt16(fields[0], 16);
	var payload = Convert.FromHexString(fields[1]);
	var expected = Convert.FromHexString(fields[2]);
	if (operation == WalletHmac.OwnershipOperation)
	{
		using var key = new Key(payload[..32]);
		var scriptBytes = payload[32..];
		var originalScript = scriptBytes.ToArray();
		var result = new OwnershipIdentifier(key, new Script(scriptBytes));
		Equal(expected, result.Bytes);
		Equal(originalScript, scriptBytes);
		Equal(payload[..32], key.ToBytes());
		if (result.Bytes.Length != OwnershipIdentifier.OwnershipIdLength) { throw new Exception("Ownership MAC was truncated."); }
		calls++;
	}
	else if (operation == WalletHmac.Slip21SeedOperation)
	{
		var originalSeed = payload.ToArray();
		var result = Slip21Node.FromSeed(payload);
		Equal(expected, NodeBytes(result));
		Equal(originalSeed, payload);
		calls++;
	}
	else if (operation == WalletHmac.Slip21ChildOperation)
	{
		var original = new byte[64];
		payload.AsSpan(0, 32).CopyTo(original);
		original[63] = 1;
		var snapshot = original.ToArray();
		var parent = new Slip21Node(original);
		var result = parent.DeriveChild(payload[32..]);
		Equal(expected, NodeBytes(result));
		Equal(snapshot, NodeBytes(parent));
		calls++;
	}
	else { throw new Exception("Unexpected synthetic operation."); }
	Array.Clear(payload);
}

// Original public SLIP21/SLIP19 paths, including the retained ASCII string API.
var seed = Convert.FromHexString("c76c4ac4f4e4a00d6b274d5c39c700bb4a7ddc04fbc6f78e85ca75007b5b495f74a9043eeb77bdd53aa6fc3a0e31462270316fa04b8c19114c8798706cd02ac8");
var master = Slip21Node.FromSeed(seed);
Equal(Convert.FromHexString("dbf12b44133eaab506a740f6565cc117228cbf1dd70635cfa8ddfdc9af734756"), NodeBytes(master)[32..]);
var branch = master.DeriveChild("SLIP-0021");
Equal(Convert.FromHexString("1d065e3ac1bbe5c7fad32cf2305f7d709dc070d672044a19e610c77cdf33de0d"), NodeBytes(branch)[32..]);
Equal(Convert.FromHexString("ea163130e35bbafdf5ddee97a17b39cef2be4b4f390180d65b54cf05c6a82fde"), NodeBytes(branch.DeriveChild("Master encryption key"))[32..]);
Equal(Convert.FromHexString("47194e938ab24cc82bfa25f6486ed54bebe79c40ae2a5a32ea6db294d81861a6"), NodeBytes(branch.DeriveChild("Authentication key"))[32..]);
var identification = master.DeriveChild("SLIP-0019").DeriveChild("Ownership identification key");
using (var identificationKey = identification.Key)
{
	var identifier = new OwnershipIdentifier(identificationKey, new Script(Convert.FromHexString("0014b2f771c370ccf219cd3059cda92bdf7f00cf2103")));
	Equal(Convert.FromHexString("a122407efc198211c81af4450f40b235d54775efd934d16b9e31c6ce9bad5707"), identifier.Bytes);
}
foreach (var label in new[] { "", "\0raw\0label", "café", "你好", "🦀", "\uD800" })
{
	Equal(NodeBytes(master.DeriveChild(Encoding.ASCII.GetBytes(label))), NodeBytes(master.DeriveChild(label)));
}
calls += 19;

// Malformed requests are rejected by the actual Rust dispatcher; the following
// valid request proves the same connection remains usable after rejection.
foreach (var operation in new[] { WalletHmac.OwnershipOperation, WalletHmac.Slip21ChildOperation })
{
	try
	{
		await McwApplicationServices.Current.RequestAsync(operation, Encoding.ASCII.GetBytes("SYNTHETIC_PRIVATE_MARKER"));
		throw new Exception("Malformed request was accepted.");
	}
	catch (IOException error)
	{
		if (error.ToString().Contains("SYNTHETIC_PRIVATE_MARKER", StringComparison.Ordinal)) { throw new Exception("Private bytes entered diagnostics."); }
	}
}
Equal(NodeBytes(master), NodeBytes(Slip21Node.FromSeed(seed)));
calls++;

// In-flight cancellation uses the production transport. No response is silently
// computed by a managed HMAC implementation. A later valid call must still work.
var canceled = 0;
for (var attempt = 0; attempt < 8 && canceled == 0; attempt++)
{
	using var cancellation = new CancellationTokenSource();
	var pending = McwApplicationServices.Current.RequestAsync(WalletHmac.Slip21SeedOperation, new byte[WalletHmac.MaxRequestBytes], cancellation.Token);
	cancellation.Cancel();
	try { Array.Clear(await pending); }
	catch (OperationCanceledException) { canceled++; }
}
if (canceled == 0) { throw new Exception("No in-flight request observed cancellation."); }
Equal(NodeBytes(master), NodeBytes(Slip21Node.FromSeed(seed)));
calls++;
Array.Clear(seed);
File.WriteAllText(report, JsonSerializer.Serialize(new { fixtureCases = 441, actualDomainCalls = calls, publicVectors = true, asciiLabels = 6, malformedRejected = 2, canceledRequests = canceled, connectionSurvived = true, adapterFailureChecks = faultChecks, callerBoundaryChecks, transportFaultChecks = transportChecks }));
return 0;

static byte[] NodeBytes(Slip21Node node) =>
	(byte[])(typeof(Slip21Node).GetField("_data", BindingFlags.Instance | BindingFlags.NonPublic)?.GetValue(node)
		?? throw new Exception("Retained node data could not be inspected."));

static void Equal(ReadOnlySpan<byte> expected, ReadOnlySpan<byte> actual)
{
	if (!expected.SequenceEqual(actual)) { throw new Exception("Synthetic compatibility output mismatch."); }
}

static int CallerBoundaryChecks()
{
	// Fail the real domain calls at the service boundary. A retained managed HMAC
	// implementation would return normally and fail these checks.
	using var key = new Key(Enumerable.Repeat((byte)1, 32).ToArray());
	var parent = new Slip21Node(new byte[64]);
	using var fault = new FailureService("failure");
	using var binding = McwApplicationServices.Bind(fault);
	foreach (var call in new Action[]
	{
		() => _ = new OwnershipIdentifier(key, new Script(new byte[] { 0 })),
		() => _ = Slip21Node.FromSeed(new byte[] { 1 }),
		() => _ = parent.DeriveChild(new byte[] { 1 })
	})
	{
		try { call(); throw new Exception("Domain caller bypassed the host boundary."); }
		catch (IOException) { }
		if (fault.Captured is not { } captured || captured.Span.IndexOfAnyExcept((byte)0) >= 0) { throw new Exception("Domain adapter request was not cleared."); }
	}
	if (fault.Requests != 3) { throw new Exception("Domain calls did not use the host."); }
	return 3;
}

static int AdapterFailureChecks()
{
	var marker = Encoding.ASCII.GetBytes("SYNTHETIC_PRIVATE_MARKER");
	using (var canceled = new CancellationTokenSource())
	{
		canceled.Cancel();
		try { WalletHmac.DeriveSlip21Seed(marker, canceled.Token); throw new Exception("Pre-cancellation was ignored."); }
		catch (OperationCanceledException) { }
	}
	try { WalletHmac.DeriveSlip21Seed(marker); throw new Exception("Unbound service was accepted."); }
	catch (InvalidOperationException) { }
	var checks = 2;
	foreach (var mode in new[] { "failure", "short", "long", "stopped" })
	{
		using var fault = new FailureService(mode);
		using var binding = McwApplicationServices.Bind(fault);
		try { WalletHmac.DeriveSlip21Seed(marker); throw new Exception("Fault response was accepted."); }
		catch (Exception error) when (error is IOException or OperationCanceledException)
		{
			if (error.ToString().Contains("SYNTHETIC_PRIVATE_MARKER", StringComparison.Ordinal)) { throw new Exception("Private bytes entered fault diagnostics."); }
		}
		if (fault.Captured is { } captured && captured.Span.IndexOfAnyExcept((byte)0) >= 0) { throw new Exception("Adapter request copy was not cleared."); }
		if (fault.Reply is { } reply && reply.AsSpan().IndexOfAnyExcept((byte)0) >= 0) { throw new Exception("Rejected response was not cleared."); }
		if (mode == "stopped" && fault.Requests != 0) { throw new Exception("Stopped host received a request."); }
		checks++;
	}
	Array.Clear(marker);
	return checks;
}

// Fault-only admission/lifetime double, never a hash implementation or fallback.
sealed class FailureService : IMcwApplicationServices, IDisposable
{
	private readonly string _mode;
	private readonly CancellationTokenSource _stop = new();
	public FailureService(string mode) { _mode = mode; if (mode == "stopped") { _stop.Cancel(); } }
	public CancellationToken Stopped => _stop.Token;
	public ReadOnlyMemory<byte>? Captured { get; private set; }
	public byte[]? Reply { get; private set; }
	public int Requests { get; private set; }
	public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
	{
		Requests++;
		Captured = payload;
		if (_mode == "failure") { return Task.FromException<byte[]>(new IOException("Synthetic host disconnect.")); }
		var reply = Enumerable.Repeat((byte)0x53, _mode == "short" ? 63 : 65).ToArray();
		Reply = reply;
		return Task.FromResult(reply);
	}
	public void Dispose() => _stop.Dispose();
}
