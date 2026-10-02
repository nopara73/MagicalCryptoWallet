using System;
using System.Threading;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Mcw;

/// <summary>Temporary service boundary. Typed adapters own their operation payloads.</summary>
public interface IMcwApplicationServices
{
    CancellationToken Stopped { get; }
    Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default);
}

/// <summary>One application service connection, bound by the application host adapter.</summary>
public static class McwApplicationServices
{
    private static IMcwApplicationServices? _current;
    public static IMcwApplicationServices Current => Volatile.Read(ref _current)
        ?? throw new InvalidOperationException("The mcw application service connection is unavailable.");

    public static IDisposable Bind(IMcwApplicationServices service)
    {
        ArgumentNullException.ThrowIfNull(service);
        if (Interlocked.CompareExchange(ref _current, service, null) is not null)
        { throw new InvalidOperationException("The mcw application service connection is already bound."); }
        return new Registration(service);
    }

    private sealed class Registration(IMcwApplicationServices service) : IDisposable
    {
        public void Dispose() => Interlocked.CompareExchange(ref _current, null, service);
    }
}
