using System;
using System.Buffers.Binary;
using System.IO;
using System.Net;
using System.Threading;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Mcw.Network;

public enum SocksProbeFailure : byte
{
    None = 0, Io = 1, TimedOut = 2, InvalidVersion = 3, MethodRejected = 4,
    UnexpectedEof = 5, Closed = 6, Cancelled = 7, Protocol = 8
}

public readonly record struct SocksProbeResult(bool IsReady, SocksProbeFailure Failure);

/// <summary>Typed transitional adapter; Rust owns the socket and SOCKS bytes.</summary>
public static class McwSocksProbe
{
    public const ushort Operation = 0x0805;

    public static async Task<SocksProbeResult> CheckAsync(EndPoint endpoint, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        if (endpoint is not IPEndPoint proxy || !IPAddress.IsLoopback(proxy.Address) ||
            proxy.Port is < 1 or > ushort.MaxValue || proxy.Address.IsIPv4MappedToIPv6 ||
            (proxy.Address.AddressFamily == System.Net.Sockets.AddressFamily.InterNetworkV6 && proxy.Address.ScopeId != 0))
        {
            throw new InvalidOperationException("The Tor SOCKS5 probe requires a literal loopback endpoint.");
        }
        var address = proxy.Address.GetAddressBytes();
        if (address.Length is not (4 or 16)) { throw new InvalidOperationException("Unsupported Tor SOCKS5 endpoint."); }
        var payload = new byte[address.Length + 4];
        payload[0] = 1;
        payload[1] = address.Length == 4 ? (byte)1 : (byte)4;
        address.CopyTo(payload, 2);
        BinaryPrimitives.WriteUInt16BigEndian(payload.AsSpan(address.Length + 2), (ushort)proxy.Port);
        var response = await McwApplicationServices.Current.RequestAsync(Operation, payload, cancellationToken).ConfigureAwait(false);
        if (response.Length != 3 || response[0] != 1 || response[1] > 1 || response[2] > 8 ||
            (response[1] == 1 && response[2] != 0) || (response[1] == 0 && response[2] == 0))
        {
            throw new IOException("Invalid SOCKS5 readiness response.");
        }
        var failure = (SocksProbeFailure)response[2];
        if (failure == SocksProbeFailure.Cancelled) { throw new OperationCanceledException(cancellationToken); }
        return new SocksProbeResult(response[1] == 1, failure);
    }
}
