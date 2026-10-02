global using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Serialization;
using MagicalCryptoWallet.Rpc;
using MagicalCryptoWallet.Services.Terminate;

// Developer test child launched by the real mcw daemon host. No wallets, keys,
// network peers, money or public RPC listener are used; HTTP stays on loopback.
using var host = ManagedApplicationHost.Connect();
if (typeof(JsonRpcRequest).GetProperty(nameof(JsonRpcRequest.Parameters))!.PropertyType != typeof(RpcValue))
{
	throw new InvalidOperationException("The bounded RPC caller patch is required for this developer probe. Run verify-json-rpc.py on its isolated candidate.");
}
var handler = new JsonRpcRequestHandler<SyntheticRpc>(new SyntheticRpc(), Network.Main);
var testable = new JsonRpcRequestHandler<MagicalCryptoWallet.Tests.TestableRpcService>(new(), Network.Main);
var single = args.Length > 2 && args[2] == "single";
var rows = File.ReadAllLines(args[0]);
var failures = new List<string>();
var responses = new List<string>();
foreach (var (row, index) in rows.Select((row, index) => (row, index)))
{
	var columns = row.Split('\t');
	var input = Encoding.UTF8.GetString(Convert.FromBase64String(columns[0]));
	var expected = Encoding.UTF8.GetString(Convert.FromBase64String(columns[1]));
	string actual;
	try { actual = columns.Length > 2 && columns[2] == "testable"
		? await testable.HandleAsync("/synthetic", input, CancellationToken.None)
		: await handler.HandleAsync("/synthetic", input, CancellationToken.None); }
	catch (Exception error) { actual = "THROW:" + error.GetType().Name + ":" + error.Message; }
	responses.Add(columns[0] + "\t" + Convert.ToBase64String(Encoding.UTF8.GetBytes(actual)));
	if (!single && actual != expected) { failures.Add($"Fixture {index}: expected {expected}; received {actual}"); }
}
File.WriteAllLines(args[1] + ".tsv", responses, new UTF8Encoding(false));
if (single) { return 0; }

// Exercise chunked input/output beyond a single bridge frame and concurrent callers.
var large = new string('x', 700_000);
var chunked = await handler.HandleAsync("/", "{\"method\":\"echo\",\"id\":9007199254740993,\"params\":[\"" + large + "\"]}", CancellationToken.None);
Check(chunked == "{\"jsonrpc\":\"2.0\",\"result\":\"" + large + "\",\"id\":\"9007199254740993\"}", "Chunked exact output");
var simultaneous = await Task.WhenAll(Enumerable.Range(0, 12).Select(index => handler.HandleAsync("/", $"{{\"method\":\"integer\",\"id\":{index},\"params\":[{index}]}}", CancellationToken.None)));
Check(simultaneous.Length == 12 && simultaneous.All(response => response.Contains("\"result\":", StringComparison.Ordinal)), "Concurrent bridge requests");
using (var cancel = new CancellationTokenSource())
{
	cancel.Cancel();
	try { await RpcJson.ParseRpcAsync("{}", cancel.Token); failures.Add("Cancelled parse completed"); }
	catch (OperationCanceledException) { }
}
using (var tooLarge = new MemoryStream(new byte[RpcJson.MaxJsonBytes + 1], writable: false))
{
	try { await JsonRpcRequest.ParseAsync(tooLarge); failures.Add("Oversized stream accepted"); }
	catch (RpcJsonException) { }
}
// Failed transformations and cancellation must release transfer slots.
for (var repeat = 0; repeat < 20; repeat++)
{
	var error = await handler.HandleAsync("/", "{broken", CancellationToken.None);
	Check(error.Contains("-32700", StringComparison.Ordinal), "Malformed input error");
}
Check((await handler.HandleAsync("/", "{\"method\":\"void\",\"id\":1}", CancellationToken.None)).Contains("\"result\":null", StringComparison.Ordinal), "Recovery after rejected transfers");
// Protocol errors remain recoverable on the actual bridge.
var service = (IMcwApplicationServices)host;
try { await service.RequestAsync(0x0101, new byte[12]); failures.Add("Unknown transfer accepted"); }
catch (IOException) { }
var transfer = await service.RequestAsync(0x0100, new byte[] { 0, 0, 0, 0, 0 });
Check(transfer.Length == 8 && BinaryPrimitives.ReadUInt64LittleEndian(transfer) != 0, "Fresh transfer identifier");
await service.RequestAsync(0x0104, transfer);

await CheckHttpAsync();
host.Dispose();
var disconnectedFailsClosed = false;
try { await handler.HandleAsync("/", "{\"method\":\"void\",\"id\":1}", CancellationToken.None); failures.Add("RPC continued after host disconnect"); }
catch (InvalidOperationException) { disconnectedFailsClosed = true; }
File.WriteAllText(args[1], System.Text.Json.JsonSerializer.Serialize(new { fixtures = rows.Length, failures, chunkBytes = large.Length, http = true, disconnectedFailsClosed }));
foreach (var failure in failures) { Console.Error.WriteLine(failure); }
return failures.Count == 0 ? 0 : 1;

void Check(bool condition, string label) { if (!condition) { failures.Add(label); } }
async Task CheckHttpAsync()
{
	using var portPicker = new TcpListener(IPAddress.Loopback, 0);
	portPicker.Start(); var port = ((IPEndPoint)portPicker.LocalEndpoint).Port; portPicker.Stop();
	var termination = new TerminateService(() => Task.CompletedTask, () => { });
	using var server = new JsonRpcServer(new SyntheticRpc(), new JsonRpcServerConfiguration(true, "synthetic", "test-only", [$"http://localhost:{port}/"], Network.Main), termination);
	await server.StartAsync(CancellationToken.None);
	using var httpHandler = new SocketsHttpHandler { UseProxy = false };
	using var client = new HttpClient(httpHandler, disposeHandler: false) { BaseAddress = new Uri($"http://localhost:{port}"), Timeout = TimeSpan.FromSeconds(10) };
	try
	{
		using var rejected = await PostAsync("/", "{\"method\":\"void\",\"id\":1}");
		Check(rejected.StatusCode == HttpStatusCode.Unauthorized, "Basic authentication rejects unauthenticated request");
		client.DefaultRequestHeaders.Authorization = new("Basic", Convert.ToBase64String(Encoding.ASCII.GetBytes("synthetic:test-only")));
		using var result = await PostAsync("/synthetic", "{\"method\":\"shapes\",\"id\":1}");
		Check(result.StatusCode == HttpStatusCode.OK && result.Content.Headers.ContentType?.MediaType == "application/json-rpc", "HTTP response headers");
		Check((await result.Content.ReadAsStringAsync()).Contains("\"long\":9007199254740993", StringComparison.Ordinal), "HTTP exact integer result");
		using var method = await client.GetAsync("/");
		Check(method.StatusCode == HttpStatusCode.MethodNotAllowed, "HTTP method restriction");
		using var notification = await PostAsync("/", "{\"method\":\"void\"}");
		Check((await notification.Content.ReadAsStringAsync()).Length == 0, "HTTP notification emits no body");
		using var malformed = await PostAsync("/", "{broken");
		Check((await malformed.Content.ReadAsStringAsync()).Contains("-32700", StringComparison.Ordinal), "HTTP malformed request error");
		using var stopped = await PostAsync("/", "{\"method\":\"stop\",\"id\":1}");
		await termination.ForcefulTerminationRequestedTask.WaitAsync(TimeSpan.FromSeconds(5));
		Check((await stopped.Content.ReadAsStringAsync()).Length == 0, "HTTP stop command retains termination behavior");
	}
	finally { await server.StopAsync(CancellationToken.None); }
	async Task<HttpResponseMessage> PostAsync(string path, string body)
	{
		using var content = new StringContent(body);
		return await client.PostAsync(path, content);
	}
}
