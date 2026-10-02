using System;
using System.Buffers.Binary;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Services.Terminate;
using MagicalCryptoWallet.Mcw;

namespace MagicalCryptoWallet.Client.Application;

/// <summary>The temporary managed adapter to the permanent mcw application host.</summary>
public sealed class ManagedApplicationHost : IDisposable, IMcwApplicationServices
{
    public const ushort QrOperation = 1;
    public const ushort ShutdownOperation = 2;
    public const ushort RestartOperation = 3;
    public const ushort UpdateOperation = 4;
    public const ushort CrashOperation = 5;
    private const int MaxFrame = 1024 * 1024;
    private const int HeaderSize = 16;
    private static readonly UTF8Encoding Utf8 = new(false, true);
    private readonly Stream _input;
    private readonly Stream _output;
    private readonly SemaphoreSlim _writeLock = new(1);
    private readonly SemaphoreSlim _requestSlots = new(256, 256);
    private readonly ConcurrentDictionary<ulong, Pending> _pending = new();
    private readonly TaskCompletionSource<byte[]> _handshake = new(TaskCreationOptions.RunContinuationsAsynchronously);
    private readonly CancellationTokenSource _stop = new();
    private long _nextId;
    private bool _disposed;
    private Action? _terminate;
    private IDisposable? _serviceBinding;
    private record Pending(ushort Operation, TaskCompletionSource<byte[]> Completion);

    public static ManagedApplicationHost? Current { get; private set; }
    public string[] StartupArguments { get; private set; } = [];
    public CancellationToken Stopped => _stop.Token;

    private ManagedApplicationHost(Stream input, Stream output)
    {
        _input = input;
        _output = output;
    }

    /// <summary>Old executable names remain compatible, but always launch mcw.</summary>
    public static bool TryDelegate(string mode, string[] arguments, out int exitCode)
    {
        exitCode = 0;
        if (Environment.GetEnvironmentVariable("MCW_HOSTED") == "1")
        {
            return false;
        }
        var executable = Path.Combine(AppContext.BaseDirectory, OperatingSystem.IsWindows() ? "mcw.exe" : "mcw");
        if (!File.Exists(executable))
        {
            Console.Error.WriteLine("mcw is missing. Build the Rust application host before launching the application.");
            exitCode = 1;
            return true;
        }
        var start = new ProcessStartInfo(executable) { UseShellExecute = false };
        start.ArgumentList.Add(mode);
        foreach (var argument in arguments) { start.ArgumentList.Add(argument); }
        // A compatibility launcher must not pass the host marker to a new root.
        start.Environment.Remove("MCW_HOSTED");
        start.Environment.Remove("MCW_HOST_PATH");
        using var process = Process.Start(start) ?? throw new IOException("Unable to start mcw.");
        process.WaitForExit();
        exitCode = process.ExitCode;
        return true;
    }

    public static ManagedApplicationHost Connect()
    {
        if (Current is not null) { throw new InvalidOperationException("The application already has a host connection."); }
        var host = new ManagedApplicationHost(Console.OpenStandardInput(), Console.OpenStandardOutput());
        // Only the binary protocol may use stdout. Capture the stream first.
        Console.SetOut(Console.Error);
        Current = host;
        _ = Task.Run(host.ReadLoopAsync);
        try
        {
            host.WriteAsync(1, 0, 0, [], CancellationToken.None).GetAwaiter().GetResult();
            var bootstrap = host._handshake.Task.WaitAsync(TimeSpan.FromSeconds(15)).GetAwaiter().GetResult();
            if (bootstrap.Length != 0) { host.StartupArguments = DecodeStrings(bootstrap); }
            host._serviceBinding = McwApplicationServices.Bind(host);
            return host;
        }
        catch
        {
            host.Dispose();
            throw;
        }
    }

    public void BindTermination(TerminateService service)
    {
        BindShutdown(service.SignalForceTerminate);
    }

    public void BindShutdown(Action terminate)
    {
        _terminate = terminate;
        if (_stop.IsCancellationRequested) { _terminate(); }
    }

    public async Task<bool[,]> GenerateQrAsync(string text, byte correction = 1, CancellationToken cancellationToken = default)
    {
        var bytes = Utf8.GetBytes(text);
        if (bytes.Length > MaxFrame - HeaderSize - 1) { throw new ArgumentException("QR content exceeds the bridge limit.", nameof(text)); }
        var payload = new byte[bytes.Length + 1];
        payload[0] = correction;
        bytes.CopyTo(payload, 1);
        var result = await ((IMcwApplicationServices)this).RequestAsync(QrOperation, payload, cancellationToken).ConfigureAwait(false);
        if (result.Length < 4) { Fail(new IOException("Invalid QR response.")); throw new IOException("Invalid QR response."); }
        var version = result[0];
        var width = BinaryPrimitives.ReadUInt16LittleEndian(result.AsSpan(2));
        if (version is < 1 or > 40 || result[1] != correction || width != version * 4 + 17 || result.Length != width * width + 4)
        {
            Fail(new IOException("Invalid QR response."));
            throw new IOException("Invalid QR response.");
        }
        var matrix = new bool[width, width];
        for (var y = 0; y < width; y++)
        {
            for (var x = 0; x < width; x++)
            {
                var module = result[4 + y * width + x];
                if (module > 1) { Fail(new IOException("Invalid QR module.")); throw new IOException("Invalid QR module."); }
                matrix[x, y] = module == 1;
            }
        }
        return matrix;
    }

    public void Handoff(ushort operation, string[] arguments) =>
        RequestAsync(operation, EncodeStrings(arguments), CancellationToken.None).GetAwaiter().GetResult();

    Task<byte[]> IMcwApplicationServices.RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken)
    {
        if (operation != QrOperation && operation < 0x0100)
        { throw new ArgumentOutOfRangeException(nameof(operation), "Application lifecycle operations are reserved."); }
        if (payload.Length > MaxFrame - HeaderSize) { throw new IOException("Application frame exceeds the limit."); }
        return RequestAsync(operation, payload.ToArray(), cancellationToken);
    }

    private async Task<byte[]> RequestAsync(ushort operation, byte[] payload, CancellationToken cancellationToken)
    {
        cancellationToken.ThrowIfCancellationRequested();
        _stop.Token.ThrowIfCancellationRequested();
        if (!_requestSlots.Wait(0, cancellationToken)) { throw new IOException("Too many outstanding application requests."); }
        var id = (ulong)Interlocked.Increment(ref _nextId);
        var source = new TaskCompletionSource<byte[]>(TaskCreationOptions.RunContinuationsAsynchronously);
        if (!_pending.TryAdd(id, new Pending(operation, source)))
        { _requestSlots.Release(); throw new IOException("Duplicate application request."); }
        try
        {
            await WriteAsync(3, id, operation, payload, cancellationToken).ConfigureAwait(false);
            return await source.Task.WaitAsync(TimeSpan.FromSeconds(30), cancellationToken).ConfigureAwait(false);
        }
        finally
        {
            if (_pending.TryRemove(id, out _) && (operation == QrOperation || operation >= 0x0100) && !_stop.IsCancellationRequested)
            {
                // The caller stops waiting immediately; the reader still drains late replies.
                _ = CancelAsync(id, operation);
            }
            _requestSlots.Release();
        }
    }

    private async Task CancelAsync(ulong id, ushort operation)
    {
        try { await WriteAsync(5, id, operation, [], _stop.Token).ConfigureAwait(false); }
        catch (Exception error) { Fail(error); }
    }

    private async Task WriteAsync(byte kind, ulong id, ushort operation, byte[] payload, CancellationToken cancellationToken)
    {
        if (payload.Length > MaxFrame - HeaderSize) { throw new IOException("Application frame exceeds the limit."); }
        var bytes = new byte[4 + HeaderSize + payload.Length];
        BinaryPrimitives.WriteInt32LittleEndian(bytes, bytes.Length - 4);
        BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(4), 1);
        bytes[6] = kind;
        BinaryPrimitives.WriteUInt64LittleEndian(bytes.AsSpan(8), id);
        BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(16), operation);
        payload.CopyTo(bytes, 20);
        await _writeLock.WaitAsync(cancellationToken).ConfigureAwait(false);
        try
        {
            // Cancellation must not leave a partial frame in the persistent stream.
            await _output.WriteAsync(bytes, _stop.Token).ConfigureAwait(false);
            await _output.FlushAsync(_stop.Token).ConfigureAwait(false);
        }
        catch (Exception error) { Fail(error); throw; }
        finally { _writeLock.Release(); }
    }

    private async Task ReadLoopAsync()
    {
        try
        {
            while (!_stop.IsCancellationRequested)
            {
                var prefix = new byte[4];
                await _input.ReadExactlyAsync(prefix, _stop.Token).ConfigureAwait(false);
                var length = BinaryPrimitives.ReadInt32LittleEndian(prefix);
                if (length is < HeaderSize or > MaxFrame) { throw new IOException("Invalid application frame length."); }
                var frame = new byte[length];
                await _input.ReadExactlyAsync(frame, _stop.Token).ConfigureAwait(false);
                if (BinaryPrimitives.ReadUInt16LittleEndian(frame) != 1 || frame[3] != 0 || frame[14] != 0 || frame[15] != 0)
                {
                    throw new IOException("Application protocol mismatch.");
                }
                var kind = frame[2];
                var id = BinaryPrimitives.ReadUInt64LittleEndian(frame.AsSpan(4));
                var operation = BinaryPrimitives.ReadUInt16LittleEndian(frame.AsSpan(12));
                var payload = frame[HeaderSize..];
                if (kind == 2 && id == 0 && operation == 0 && !_handshake.Task.IsCompleted)
                {
                    _handshake.TrySetResult(payload);
                }
                else if (kind == 3 && id == 0 && operation == ShutdownOperation && payload.Length == 0)
                {
                    Fail(new IOException("The application host requested shutdown."));
                    return;
                }
                else if (kind is 2 or 4 && id != 0)
                {
                    IOException? serviceError = null;
                    if (kind == 4)
                    {
                        if (payload.Length < 2) { throw new IOException("Invalid application error."); }
                        var code = BinaryPrimitives.ReadUInt16LittleEndian(payload);
                        if (code == 0) { throw new IOException("Invalid application error code."); }
                        // Validate before removing the pending request, so malformed UTF-8
                        // also fails that caller immediately through the disconnect path.
                        serviceError = new IOException($"mcw service error {code}: {Utf8.GetString(payload, 2, payload.Length - 2)}");
                    }
                    if (_pending.TryRemove(id, out var pending))
                    {
                        if (operation != pending.Operation) { pending.Completion.TrySetException(new IOException("Application operation mismatch.")); throw new IOException("Application operation mismatch."); }
                        if (kind == 2) { pending.Completion.TrySetResult(payload); }
                        else { pending.Completion.TrySetException(serviceError!); }
                    }
                    // A canceled caller has released this ID; its response is drained.
                }
                else { throw new IOException("Unexpected application message."); }
            }
        }
        catch (Exception error) { if (!_disposed) { Fail(error); } }
    }

    private void Fail(Exception error)
    {
        _handshake.TrySetException(error);
        foreach (var pair in _pending) { if (_pending.TryRemove(pair.Key, out var pending)) { pending.Completion.TrySetException(new IOException("The mcw application service disconnected.", error)); } }
        if (!_stop.IsCancellationRequested)
        {
            _stop.Cancel();
            _terminate?.Invoke();
        }
    }

    private static byte[] EncodeStrings(string[] arguments)
    {
        if (arguments.Length > 256) { throw new IOException("Too many lifecycle arguments."); }
        using var output = new MemoryStream();
        using var writer = new BinaryWriter(output, Utf8, leaveOpen: true);
        writer.Write(arguments.Length);
        foreach (var argument in arguments)
        {
            if (argument.Contains('\0')) { throw new IOException("Invalid lifecycle argument."); }
            var bytes = Utf8.GetBytes(argument);
            writer.Write(bytes.Length); writer.Write(bytes);
        }
        if (output.Length > MaxFrame - HeaderSize) { throw new IOException("Lifecycle frame exceeds limit."); }
        return output.ToArray();
    }

    private static string[] DecodeStrings(byte[] payload)
    {
        using var input = new MemoryStream(payload);
        using var reader = new BinaryReader(input, Utf8);
        var count = reader.ReadInt32();
        if (count is < 0 or > 256) { throw new IOException("Invalid startup arguments."); }
        var result = new string[count];
        for (var i = 0; i < count; i++)
        {
            var length = reader.ReadInt32();
            if (length < 0 || length > input.Length - input.Position) { throw new IOException("Invalid startup argument length."); }
            result[i] = Utf8.GetString(reader.ReadBytes(length));
        }
        if (input.Position != input.Length) { throw new IOException("Trailing startup bytes."); }
        return result;
    }

    public void Dispose()
    {
        if (_disposed) { return; }
        // Normal disposal follows managed cleanup. It must not invoke an already
        // disposed termination owner or turn a successful exit into a crash.
        _terminate = null;
        if (!_stop.IsCancellationRequested && _handshake.Task.IsCompletedSuccessfully)
        {
            try { RequestAsync(ShutdownOperation, [], CancellationToken.None).GetAwaiter().GetResult(); }
            catch (Exception) { /* The host may already be closing its side. */ }
        }
        _disposed = true;
        _serviceBinding?.Dispose();
        Fail(new IOException("The managed application is stopping."));
        _input.Dispose(); _output.Dispose();
        if (ReferenceEquals(Current, this)) { Current = null; }
    }
}
