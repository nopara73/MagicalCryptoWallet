using System;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Network;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Tor;

if (args.Length != 2 || args[0] != "socks-probe-tests") { return 2; }
var report = Path.GetFullPath(args[1]);
var syntheticRoot = Path.Combine(Path.GetDirectoryName(report)!, "synthetic-probe-data");
Directory.CreateDirectory(syntheticRoot);
var checks = 0;

// The production typed adapter rejects malformed service replies and domain
// proxies before the actual host tests. This fake service opens no network.
foreach (var invalid in new byte[][] { [], [1], [1, 1], [2, 1, 0], [1, 2, 0], [1, 1, 1], [1, 0, 0], [1, 0, 9], [1, 1, 0, 0] })
{
    using var binding = McwApplicationServices.Bind(new FakeService(invalid));
    try { await McwSocksProbe.CheckAsync(new IPEndPoint(IPAddress.Loopback, 12345), CancellationToken.None); throw new Exception("Invalid reply accepted."); }
    catch (IOException) { checks++; }
}
using (var binding = McwApplicationServices.Bind(new FakeService([1, 1, 0])))
{
    foreach (EndPoint invalid in new EndPoint[] { new DnsEndPoint("example.invalid", 12345), new IPEndPoint(IPAddress.Parse("192.0.2.1"), 12345), new IPEndPoint(IPAddress.Loopback, 0), new IPEndPoint(IPAddress.Parse("::ffff:127.0.0.1"), 12345) })
    {
        try { await McwSocksProbe.CheckAsync(invalid, CancellationToken.None); throw new Exception("Invalid proxy accepted."); }
        catch (InvalidOperationException) { checks++; }
    }
}
using var host = ManagedApplicationHost.Connect();
using var stop = new CancellationTokenSource();
host.BindShutdown(stop.Cancel);

foreach (var pair in new (byte[]? Reply, bool Ready)[] { ([5, 0], true), ([5, 2], false), ([5, 255], false), ([4], false), ([5], false), ([], false), (null, false) })
{
    await RunAsync(pair.Reply, pair.Ready);
    checks++;
}

// A cancelled managed call returns promptly. The existing synchronous host
// dispatch finishes its socket within the native 250ms deadline and drains its
// late response before the next call; it does not promise immediate native abort.
var listener = new TcpListener(IPAddress.Loopback, 0);
listener.Start();
var port = ((IPEndPoint)listener.LocalEndpoint).Port;
var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
var server = Task.Run(async () => {
    using var client = await listener.AcceptTcpClientAsync();
    using var stream = client.GetStream();
    using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3));
    var greeting = new byte[3];
    await stream.ReadExactlyAsync(greeting, timeout.Token);
    if (!greeting.AsSpan().SequenceEqual(new byte[] { 5, 1, 0 })) { throw new Exception("Greeting mismatch."); }
    entered.SetResult();
    if (await stream.ReadAsync(new byte[1], timeout.Token) != 0) { throw new Exception("Unexpected CONNECT or payload."); }
});
var events = new List<bool>();
var bus = new EventBus();
using var subscription = bus.Subscribe<TorConnectionStateChanged>(item => events.Add(item.IsTorRunning));
var settings = new TorSettings(syntheticRoot, syntheticRoot, false, socksPort: port, log: false);
var manager = new TorProcessManager(settings, bus);
using (var cancellation = new CancellationTokenSource())
{
    var pending = manager.IsTorRunningAsync(cancellation.Token);
    await entered.Task.WaitAsync(TimeSpan.FromSeconds(3));
    var elapsed = Stopwatch.StartNew();
    cancellation.Cancel();
    try { await pending; throw new Exception("Cancellation ignored."); } catch (OperationCanceledException) { }
    if (elapsed.Elapsed > TimeSpan.FromSeconds(1)) { throw new Exception("Managed cancellation was not prompt."); }
    if (events.Count != 0) { throw new Exception("Cancellation published readiness."); }
}
await server;
listener.Stop();
await RunAsync([5, 0], true);
checks += 2;

using (var cancellation = new CancellationTokenSource())
{
    cancellation.Cancel();
    try { await manager.IsTorRunningAsync(cancellation.Token); throw new Exception("Pre-cancellation ignored."); } catch (OperationCanceledException) { checks++; }
}
var unavailable = new TcpListener(IPAddress.Loopback, 0);
unavailable.Start();
var unavailablePort = ((IPEndPoint)unavailable.LocalEndpoint).Port;
unavailable.Stop();
var failedBus = new EventBus();
var failureEvents = 0;
using (failedBus.Subscribe<TorConnectionStateChanged>(item => { if (item.IsTorRunning) { throw new Exception("Refusal published ready."); } failureEvents++; }))
{
    var failed = new TorProcessManager(new TorSettings(syntheticRoot, syntheticRoot, false, socksPort: unavailablePort, log: false), failedBus);
    if (await failed.IsTorRunningAsync(CancellationToken.None) || failureEvents != 1) { throw new Exception("Refusal result mismatch."); }
    checks++;
}
foreach (var invalid in new byte[][] { [], [0, 1, 127, 0, 0, 1, 0, 1], [1, 3, 1], [1, 1, 192, 0, 2, 1, 0, 1], [1, 1, 127, 0, 0, 1, 0, 0], [1, 1, 127, 0, 0, 1, 0, 1, 0] })
{
    try { await McwApplicationServices.Current.RequestAsync(McwSocksProbe.Operation, invalid); throw new Exception("Invalid native request accepted."); }
    catch (IOException) { checks++; }
}
await RunAsync([5, 0], true);
checks++;
File.WriteAllText(report, JsonSerializer.Serialize(new { checks, actualProductionCaller = "TorProcessManager.IsTorRunningAsync", noTorLaunched = true, noWalletData = true, nativeProbeDeadlineMs = 250 }));
return 0;

async Task RunAsync(byte[]? reply, bool expected)
{
    var proxy = new TcpListener(IPAddress.Loopback, 0);
    proxy.Start();
    var proxyPort = ((IPEndPoint)proxy.LocalEndpoint).Port;
    var proxyTask = Task.Run(async () => {
        using var client = await proxy.AcceptTcpClientAsync();
        using var stream = client.GetStream();
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(3));
        var greeting = new byte[3];
        await stream.ReadExactlyAsync(greeting, timeout.Token);
        if (!greeting.AsSpan().SequenceEqual(new byte[] { 5, 1, 0 })) { throw new Exception("Greeting mismatch."); }
        if (reply is not null)
        {
            foreach (var value in reply) { await stream.WriteAsync(new byte[] { value }, timeout.Token); await Task.Delay(2); }
            client.Client.Shutdown(SocketShutdown.Send);
        }
        try { if (await stream.ReadAsync(new byte[1], timeout.Token) != 0) { throw new Exception("Unexpected CONNECT or payload."); } }
        catch (IOException error) when (error.InnerException is SocketException socket && socket.SocketErrorCode is SocketError.ConnectionReset or SocketError.ConnectionAborted) { }
    });
    var localBus = new EventBus();
    var changes = new List<bool>();
    using var change = localBus.Subscribe<TorConnectionStateChanged>(item => changes.Add(item.IsTorRunning));
    var localManager = new TorProcessManager(new TorSettings(syntheticRoot, syntheticRoot, false, socksPort: proxyPort, log: false), localBus);
    var elapsed = Stopwatch.StartNew();
    if (await localManager.IsTorRunningAsync(CancellationToken.None) != expected) { throw new Exception("Readiness result mismatch."); }
    if (changes.Count != 1 || changes[0] != expected) { throw new Exception("Readiness event mismatch."); }
    if (elapsed.Elapsed > TimeSpan.FromSeconds(2)) { throw new Exception("Probe deadline exceeded."); }
    await proxyTask;
    proxy.Stop();
}

sealed class FakeService(byte[] response) : IMcwApplicationServices
{
    public CancellationToken Stopped => CancellationToken.None;
    public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default) => Task.FromResult(response);
}
