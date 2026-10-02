using System;
using System.IO;
using System.Net;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Tor;

namespace MagicalCryptoWallet.Coordinator.Tor;

/// <summary>Explicit unhosted coordinator role; wallet readiness remains Rust.</summary>
public sealed class CoordinatorTorProcessManager : TorProcessManager
{
    private readonly EndPoint _proxy;
    private readonly EventBus _eventBus;

    public CoordinatorTorProcessManager(TorSettings settings, EventBus eventBus)
        : base(settings, eventBus, readReply: CoordinatorTorControlReplyReader.ReadReplyAsync)
    {
        _proxy = settings.SocksEndpoint;
        _eventBus = eventBus;
    }

    public override async Task<bool> IsTorRunningAsync(CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        using var socket = new Socket(_proxy.AddressFamily, SocketType.Stream, ProtocolType.Tcp);
        try
        {
            await socket.ConnectAsync(_proxy, cancellationToken).ConfigureAwait(false);
            using var stream = new NetworkStream(socket, ownsSocket: false);
            await stream.WriteAsync(new byte[] { 5, 1, 0 }, cancellationToken).ConfigureAwait(false);
            var response = new byte[2];
            await stream.ReadExactlyAsync(response.AsMemory(0, 1), cancellationToken).ConfigureAwait(false);
            var ready = false;
            if (response[0] == 5)
            {
                await stream.ReadExactlyAsync(response.AsMemory(1, 1), cancellationToken).ConfigureAwait(false);
                ready = response[1] == 0;
            }
            if (!ready) { Logger.LogInfo("Coordinator Tor SOCKS5 readiness negotiation failed."); }
            _eventBus.Publish(new TorConnectionStateChanged(ready));
            return ready;
        }
        catch (System.OperationCanceledException) { throw; }
        catch (System.Exception error) when (error is SocketException or IOException)
        {
            Logger.LogInfo("Coordinator Tor SOCKS5 readiness connection failed.");
            _eventBus.Publish(new TorConnectionStateChanged(false));
            return false;
        }
    }
}
