// Development-only functional caller checks. No app/package/state owner is added.
using System.Collections.Immutable;
using System.Net;
using System.Text;
using System.Text.Json.Nodes;
using MagicalCryptoWallet.Crypto;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Network;
using MagicalCryptoWallet.Mcw.Scripts;
using MagicalCryptoWallet.Serialization;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.WabiSabi.Models;
using MagicalCryptoWallet.WabiSabi.Models.MultipartyTransaction;
using NBitcoin;
using WabiSabi.CredentialRequesting;
using WabiSabi.Crypto;
using WabiSabi.Crypto.Groups;

var nativeMode = args.Contains("--script-text-native-child", StringComparer.Ordinal);
using var nativeHost = nativeMode ? ManagedApplicationHost.Connect() : null;
var checks = 0;
void Check(bool condition, string name)
{
    if (!condition) { throw new InvalidOperationException("Failed: " + name); }
    checks++;
}
void Throws<T>(Action action, string name) where T : Exception
{
    try { action(); } catch (T) { checks++; return; }
    throw new InvalidOperationException("Expected " + typeof(T).Name + ": " + name);
}

var legacyScript = new Script(new byte[] { 0x76 });
var credentials = new RealCredentialsRequest(0, [], [], []);
var outputRequest = new OutputRegistrationRequest(uint256.One, legacyScript, credentials, credentials);
var g = Generators.FromText("synthetic-script-text-client-check");
var issuer = new CredentialIssuerParameters(g, g);
var coin = new Coin(new OutPoint(uint256.One, 0), new TxOut(Money.Satoshis(1000), legacyScript));
var events = ImmutableList.Create<IEvent>(new InputAdded(coin, new OwnershipProof()), new OutputAdded(new TxOut(Money.Satoshis(500), legacyScript)));
var construction = new ConstructionState(null!) { Events = events };
var round = new RoundState(uint256.One, uint256.Zero, issuer, issuer, Phase.InputRegistration, EndRoundState.None, DateTimeOffset.UnixEpoch, TimeSpan.FromMinutes(1), construction);
var status = new RoundStateResponse([round]);
var statusJson = JsonEncoder.ToString(status, Encode.CoordinatorMessage);
var requestJson = JsonEncoder.ToString(outputRequest, Encode.CoordinatorMessage);

if (nativeMode)
{
    foreach (var text in new[] { "OP_UNKNOWN(0xba", "OP_UNKNOWN(0xba)extra", "OP_UNKNOWN(0xbaanything" })
    {
        Check(ScriptTextClient.Parse(text).SequenceEqual(new byte[] { 0xba }), "native permissive unknown suffix");
    }
    foreach (var text in new[] { "\vOP_DUP\v", "\u00a0OP_DUP\u00a0", "\u2003OP_DUP\u2003" })
    {
        Check(ScriptTextClient.Parse(text).SequenceEqual(legacyScript.ToBytes()), "native boundary whitespace");
    }
    Check(ScriptTextClient.Parse("OP_DUP\vOP_HASH160").SequenceEqual(new byte[] { 0x76, 0xa9 }), "native interior vertical tab");
    Check(ScriptTextClient.Render(new byte[] { 0x4e, 0xff, 0xff, 0xff, 0xff }) == "0", "native adversarial declared length");
    Check(ScriptTextClient.Render(new byte[] { 0x76, 2, 1 }) == "OP_DUP 0", "native truncated push prefix");
    Check(ScriptTextClient.Render(Array.Empty<byte>()) == "", "native empty display");
    Check(ScriptTextClient.Parse("").Length == 0, "native empty script");
    Throws<IOException>(() => ScriptTextClient.Parse("not-a-script-token"), "native parse error fails explicitly");
    Throws<IOException>(() => McwApplicationServices.Current.RequestAsync(ScriptTextClient.ParseOperation, new byte[] { 0xff }).GetAwaiter().GetResult(), "host rejects invalid request UTF-8");
    var oversizedDisplay = new byte[150_000];
    Array.Fill(oversizedDisplay, (byte)0x76);
    Throws<IOException>(() => ScriptTextClient.Render(oversizedDisplay), "host rejects oversized rendered response");
    var factory = new MemoryHttpFactory(statusJson);
    var client = new WabiSabiHttpApiClient("synthetic-script-text-native-client", factory);
    await client.RegisterOutputAsync(outputRequest, CancellationToken.None);
    Check(factory.Handler.Bodies.Single() == requestJson, "real host RegisterOutputAsync exact wire JSON");
    var parsed = await client.GetStatusAsync(RoundStateRequest.Empty, CancellationToken.None);
    Check(JsonEncoder.ToString(parsed, Encode.CoordinatorMessage) == statusJson, "real host GetStatusAsync exact decoded status");
    Check(parsed.RoundStates.Single().CoinjoinState.Events.OfType<InputAdded>().Single().Coin.TxOut.ScriptPubKey.ToBytes().SequenceEqual(legacyScript.ToBytes()), "real native parser input coin bytes");
    Check(parsed.RoundStates.Single().CoinjoinState.Events.OfType<OutputAdded>().Single().Output.ScriptPubKey.ToBytes().SequenceEqual(legacyScript.ToBytes()), "real native parser output bytes");
    Console.WriteLine($"SCRIPT_TEXT_NATIVE_CALLER_CHECKS={checks}");
    return;
}

// These tests prove routing: deliberately different fixture bytes/text must be
// used in the actual objects. They do not claim native algorithm execution;
// the separate real-host check and retained-library corpus provide that evidence.
var fixture = new RecordingServices((op, input) => op switch
{
    ScriptTextClient.ParseOperation => new byte[] { 0x51 },
    ScriptTextClient.RenderOperation => Encoding.UTF8.GetBytes("OP_VERIFY"),
    _ => throw new InvalidOperationException("Unexpected service operation")
});
using (McwApplicationServices.Bind(fixture))
{
    var clientJson = ClientScriptTextJson.EncodeRequest(outputRequest);
    Check(clientJson["Script"]!.GetValue<string>() == "OP_VERIFY", "native rendered field is used");
    Check(fixture.Requests.Single().Operation == ScriptTextClient.RenderOperation, "render operation");
    Check(fixture.Requests.Single().Payload.SequenceEqual(legacyScript.ToBytes()), "render exact raw bytes");
    var parsed = ClientScriptTextJson.DecodeResponse<RoundStateResponse>(statusJson);
    var parsedEvents = parsed.RoundStates.Single().CoinjoinState.Events;
    Check(parsedEvents.OfType<InputAdded>().Single().Coin.TxOut.ScriptPubKey.ToBytes().SequenceEqual(new byte[] { 0x51 }), "native bytes reach input coin");
    Check(parsedEvents.OfType<OutputAdded>().Single().Output.ScriptPubKey.ToBytes().SequenceEqual(new byte[] { 0x51 }), "native bytes reach output");
    Check(fixture.Requests.Count(x => x.Operation == ScriptTextClient.ParseOperation) == 2, "both status script leaves use native parsing");
    Check(fixture.Requests.Where(x => x.Operation == ScriptTextClient.ParseOperation).All(x => Encoding.UTF8.GetString(x.Payload) == "OP_DUP"), "parse exact decoded text");
    var count = fixture.Requests.Count;
    var emptyStatus = ClientScriptTextJson.DecodeResponse<RoundStateResponse>("{\"roundStates\":[]}");
    Check(emptyStatus.RoundStates.Length == 0 && fixture.Requests.Count == count, "empty status has no ceremonial parse");
    ClientScriptTextJson.EncodeRequest(RoundStateRequest.Empty);
    Check(fixture.Requests.Count == count, "script-free request has no native call");
    Throws<NotSupportedException>(() => ClientScriptTextJson.EncodeRequest(status), "response rejected at request boundary");
    Throws<NotSupportedException>(() => ClientScriptTextJson.DecodeResponse<OutputRegistrationRequest>(requestJson), "request rejected at response boundary");
}

var forbidden = new RecordingServices((_, _) => throw new InvalidOperationException("Coordinator called mcw"));
using (McwApplicationServices.Bind(forbidden))
{
    Check(JsonEncoder.ToString(outputRequest, Encode.CoordinatorMessage) == requestJson, "coordinator encoding remains managed");
    Check(Decode.CoordinatorMessage<RoundStateResponse>(statusJson).RoundStates.Single().CoinjoinState.Events.OfType<OutputAdded>().Single().Output.ScriptPubKey.ToBytes().SequenceEqual(legacyScript.ToBytes()), "coordinator parsing remains managed");
    using var stream = new MemoryStream(Encoding.UTF8.GetBytes(statusJson));
    var streamed = await Decode.CoordinatorMessageFromStreamAsync(stream, typeof(RoundStateResponse));
    Check(streamed.IsOk, "external coordinator stream decoder remains usable");
    Check(forbidden.Requests.Count == 0, "coordinator paths never request native service");
}

Throws<InvalidOperationException>(() => ScriptTextClient.Render(legacyScript.ToBytes()), "unavailable service has no legacy fallback");
Throws<InvalidOperationException>(() => ClientScriptTextJson.EncodeRequest(outputRequest), "client output fails closed without service");
var failed = new RecordingServices((_, _) => throw new FormatException("synthetic native parse failure"));
using (McwApplicationServices.Bind(failed))
{
    Check(ClientScriptTextJson.DecodeResponse<RoundStateResponse>(statusJson) is null, "native parse failure never produces a legacy-decoded status");
}
var invalidText = new RecordingServices((_, _) => new byte[] { 0xff });
using (McwApplicationServices.Bind(invalidText))
{
    Throws<FormatException>(() => ScriptTextClient.Render(legacyScript.ToBytes()), "invalid response UTF-8 rejected");
    var count = invalidText.Requests.Count;
    Throws<FormatException>(() => ScriptTextClient.Render(new byte[ScriptTextClient.MaximumPayloadBytes + 1]), "frame bound before sending");
    Throws<FormatException>(() => ScriptTextClient.Parse(new string('x', ScriptTextClient.MaximumPayloadBytes + 1)), "text bound before sending");
    using var cancellation = new CancellationTokenSource();
    cancellation.Cancel();
    Throws<OperationCanceledException>(() => ScriptTextClient.Parse("OP_DUP", cancellation.Token), "cancellation before sending");
    Check(invalidText.Requests.Count == count, "rejected payloads issue no request");
}

// Exercise both retained production HTTP client methods with an in-memory HTTP
// handler. No network call, real wallet, coordinator or data directory is used.
var accurate = new RecordingServices((op, input) => op switch
{
    ScriptTextClient.ParseOperation when Encoding.UTF8.GetString(input.Span) == "OP_DUP" => legacyScript.ToBytes(),
    ScriptTextClient.RenderOperation when input.Span.SequenceEqual(legacyScript.ToBytes()) => Encoding.UTF8.GetBytes("OP_DUP"),
    _ => throw new InvalidOperationException("Unexpected actual client payload")
});
using (McwApplicationServices.Bind(accurate))
{
    var factory = new MemoryHttpFactory(statusJson);
    var client = new WabiSabiHttpApiClient("synthetic-script-text-client", factory);
    await client.RegisterOutputAsync(outputRequest, CancellationToken.None);
    Check(factory.Handler.Bodies.Single() == requestJson, "actual RegisterOutputAsync preserves JSON wire format");
    var parsed = await client.GetStatusAsync(RoundStateRequest.Empty, CancellationToken.None);
    Check(JsonEncoder.ToString(parsed, Encode.CoordinatorMessage) == statusJson, "actual GetStatusAsync preserves decoded response");
    Check(accurate.Requests.Count == 3, "actual request and response roots invoke three real script leaves");
}
Console.WriteLine($"SCRIPT_TEXT_CLIENT_ROUTING_CHECKS={checks}");

sealed class RecordingServices(Func<ushort, ReadOnlyMemory<byte>, byte[]> reply) : IMcwApplicationServices
{
    public CancellationToken Stopped => CancellationToken.None;
    public List<(ushort Operation, byte[] Payload)> Requests { get; } = [];
    public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
    {
        Requests.Add((operation, payload.ToArray()));
        try { return Task.FromResult(reply(operation, payload)); }
        catch (Exception error) { return Task.FromException<byte[]>(error); }
    }
}

sealed class MemoryHttpFactory(string statusJson) : IMcwHttpClientFactory
{
    public MemoryHandler Handler { get; } = new(statusJson);
    public HttpClient CreateClient(string name) => new(Handler, false) { BaseAddress = new Uri("https://synthetic-script-text.invalid/") };
}

sealed class MemoryHandler(string statusJson) : HttpMessageHandler
{
    public List<string> Bodies { get; } = [];
    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        Bodies.Add(await request.Content!.ReadAsStringAsync(cancellationToken));
        return new HttpResponseMessage(HttpStatusCode.OK)
        {
            Content = new StringContent(request.RequestUri!.AbsolutePath.EndsWith("/status", StringComparison.Ordinal) ? statusJson : "{}")
        };
    }
}
