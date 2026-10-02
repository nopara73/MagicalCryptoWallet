using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.IO.Pipelines;
using System.Net;
using System.Net.Sockets;
using System.Reflection;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Extensions.Configuration;
using MagicalCryptoWallet.Coordinator;
using MagicalCryptoWallet.Coordinator.Tor;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Tor;
using MagicalCryptoWallet.Tor.Control;
using MagicalCryptoWallet.WabiSabi.Coordinator;

if (args.Length != 1) { return 2; }
var report = Path.GetFullPath(args[0]);
var data = Path.Combine(Path.GetDirectoryName(report)!, "synthetic-coordinator-data");
Directory.CreateDirectory(data);
var checks = 0;
AssertUnbound();

// Wallet readiness must reject an unhosted role without opening a socket.
using (var listener = new TcpListener(IPAddress.Loopback, 0))
{
    listener.Start();
    var port = ((IPEndPoint)listener.LocalEndpoint).Port;
    var bus = new EventBus();
    var events = 0;
    using var subscription = bus.Subscribe<TorConnectionStateChanged>(_ => events++);
    var wallet = new TorProcessManager(Settings(port), bus, readReply: TorControlReplyReader.ReadReplyAsync);
    try { await wallet.IsTorRunningAsync(CancellationToken.None); throw new Exception("Unhosted wallet opened a managed probe."); }
    catch (InvalidOperationException) { }
    if (listener.Pending() || events != 0) { throw new Exception("Unhosted wallet socket/event leakage."); }
    checks++;
}

// Inspect the real hosted-service construction without starting Tor or ASP.NET.
var service = new TorManagerService(Settings(12345), new WabiSabiConfig(), new ConfigurationBuilder().Build());
var manager = (TorManager)typeof(TorManagerService).GetField("_torManager", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(service)!;
var process = (TorProcessManager)typeof(TorManager).GetField("_processManager", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(manager)!;
var reader = (Delegate)typeof(TorProcessManager).GetField("_readReply", BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(process)!;
if (process is not CoordinatorTorProcessManager || reader.Method.DeclaringType != typeof(CoordinatorTorControlReplyReader)) {
    throw new Exception("External coordinator construction lost its explicit readiness/parser role.");
}
checks++;

var pipe = new Pipe();
await pipe.Writer.WriteAsync("250 OK\r\n"u8.ToArray());
await pipe.Writer.CompleteAsync();
var reply = await CoordinatorTorControlReplyReader.ReadReplyAsync(pipe.Reader, CancellationToken.None);
if ((int)reply.StatusCode != 250 || reply.ResponseLines.Count != 1 || reply.ResponseLines[0] != "OK") { throw new Exception("Unhosted coordinator parser required Rust."); }
await pipe.Reader.CompleteAsync();
checks++;

foreach (var pair in new (byte[] Reply, bool Ready)[] { ([5, 0], true), ([5, 2], false), ([5, 255], false), ([4], false), ([5], false), ([], false), ([5, 1], false) }) {
    await ProbeAsync(pair.Reply, pair.Ready);
    checks++;
}

using (var cancellation = new CancellationTokenSource()) {
    cancellation.Cancel();
    try { await new CoordinatorTorProcessManager(Settings(12345), new EventBus()).IsTorRunningAsync(cancellation.Token); throw new Exception("Pre-cancellation ignored."); }
    catch (OperationCanceledException) { checks++; }
}

using (var listener = new TcpListener(IPAddress.Loopback, 0)) {
    listener.Start();
    var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
    var server = Task.Run(async () => {
        using var connection = await listener.AcceptTcpClientAsync();
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        using var stream = connection.GetStream();
        var greeting = new byte[3];
        await stream.ReadExactlyAsync(greeting, timeout.Token);
        if (!greeting.AsSpan().SequenceEqual(new byte[] { 5, 1, 0 })) { throw new Exception("Coordinator greeting changed."); }
        entered.SetResult();
        if (await stream.ReadAsync(new byte[1], timeout.Token) != 0) { throw new Exception("Coordinator sent CONNECT/data."); }
    });
    var bus = new EventBus();
    var events = 0;
    using var subscription = bus.Subscribe<TorConnectionStateChanged>(_ => events++);
    using var cancellation = new CancellationTokenSource();
    TorProcessManager external = new CoordinatorTorProcessManager(Settings(((IPEndPoint)listener.LocalEndpoint).Port), bus);
    var pending = external.IsTorRunningAsync(cancellation.Token);
    await entered.Task.WaitAsync(TimeSpan.FromSeconds(3));
    var elapsed = Stopwatch.StartNew();
    cancellation.Cancel();
    try { await pending; throw new Exception("Coordinator cancellation ignored."); } catch (OperationCanceledException) { }
    if (events != 0 || elapsed.Elapsed > TimeSpan.FromSeconds(1)) { throw new Exception("Coordinator cancellation sanity bound/event mismatch."); }
    await server;
    checks++;
}

var refusal = new TcpListener(IPAddress.Loopback, 0);
refusal.Start();
var refusedPort = ((IPEndPoint)refusal.LocalEndpoint).Port;
refusal.Stop();
var refusalBus = new EventBus();
var refusalEvents = 0;
using (refusalBus.Subscribe<TorConnectionStateChanged>(state => { if (state.IsTorRunning) { throw new Exception("Refusal published ready."); } refusalEvents++; })) {
    using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3));
    if (await new CoordinatorTorProcessManager(Settings(refusedPort), refusalBus).IsTorRunningAsync(timeout.Token) || refusalEvents != 1) { throw new Exception("Coordinator refusal mismatch."); }
    checks++;
}
AssertUnbound();
File.WriteAllText(report, JsonSerializer.Serialize(new { checks, applicationHostBindingAbsent = true, walletRequiresRust = true,
    explicitCoordinatorReadinessAndParser = true, noTorLaunched = true, noWalletData = true }));
return 0;

TorSettings Settings(int port) => new(data, data, false, socksPort: port, log: false);
void AssertUnbound() {
    try { _ = McwApplicationServices.Current; throw new Exception("Unexpected application-host service binding."); }
    catch (InvalidOperationException) { checks++; }
}
async Task ProbeAsync(byte[] bytes, bool expected) {
    using var listener = new TcpListener(IPAddress.Loopback, 0);
    listener.Start();
    var server = Task.Run(async () => {
        using var connection = await listener.AcceptTcpClientAsync();
        using var stream = connection.GetStream();
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        var greeting = new byte[3];
        await stream.ReadExactlyAsync(greeting, timeout.Token);
        if (!greeting.AsSpan().SequenceEqual(new byte[] { 5, 1, 0 })) { throw new Exception("Coordinator greeting changed."); }
        foreach (var value in bytes) { await stream.WriteAsync(new byte[] { value }, timeout.Token); await Task.Delay(2); }
        connection.Client.Shutdown(SocketShutdown.Send);
        try { if (await stream.ReadAsync(new byte[1], timeout.Token) != 0) { throw new Exception("Coordinator sent CONNECT/data."); } }
        catch (IOException error) when (error.InnerException is SocketException socket && socket.SocketErrorCode is SocketError.ConnectionReset or SocketError.ConnectionAborted) { }
    });
    var events = new List<bool>();
    var bus = new EventBus();
    using var subscription = bus.Subscribe<TorConnectionStateChanged>(item => events.Add(item.IsTorRunning));
    using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3));
    TorProcessManager external = new CoordinatorTorProcessManager(Settings(((IPEndPoint)listener.LocalEndpoint).Port), bus);
    if (await external.IsTorRunningAsync(timeout.Token) != expected || events.Count != 1 || events[0] != expected) { throw new Exception("Coordinator readiness/event mismatch."); }
    await server;
}
