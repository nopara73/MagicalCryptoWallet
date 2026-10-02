// Development-only: compile the real QrCodeReader + adapter. No camera is opened.
using System.Buffers.Binary;
using System.Collections.Concurrent;
using System.Diagnostics;
using System.Text;
using System.Text.Json;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Scanning;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Client.Application;
using SkiaSharp;

static class Program
{
    // The portable probe uses the actual Fluent assembly's internal capture
    // boundary, without copying that leaf or widening the application's API.
    private static readonly Func<ArraySegment<byte>, CancellationToken, Task<string>> DecodeCapturedImageAsync =
        typeof(QrCodeReader).GetMethod("DecodeCapturedImageAsync", System.Reflection.BindingFlags.Static | System.Reflection.BindingFlags.NonPublic)!
            .CreateDelegate<Func<ArraySegment<byte>, CancellationToken, Task<string>>>();
    public static async Task Main(string[] args)
    {
        VerifyDiagnosticRedaction();
        if (Environment.GetEnvironmentVariable("MCW_HOSTED") == "1")
        {
            await RunHosted(args);
            return;
        }
        await using var service = new FixtureConnection(args[0]);
        await service.Handshake();
        using var registration = McwApplicationServices.Bind(service);
        var cases = JsonSerializer.Deserialize<List<Symbol>>(File.ReadAllText(args[1]))!;
        var tested = await CheckSymbols(cases);
        var retained = await CheckRetainedImages(args[2]);
        var large = await McwQrDecoder.DecodeLuminanceAsync(1500, 900, 1500, Enumerable.Repeat((byte)255, 1350000).ToArray());
        Require(large is null, "Blank multi-chunk image became text.");
        var maximum = await McwQrDecoder.DecodeLuminanceAsync(4096, 4096, 4096, Enumerable.Repeat((byte)255, 16777216).ToArray());
        Require(maximum is null, "Maximum-resolution blank frame became text.");
        var malformed = false;
        try { await DecodeCapturedImageAsync(new ArraySegment<byte>(new byte[] { 1, 2, 3 }), CancellationToken.None); }
        catch (InvalidOperationException) { malformed = true; }
        Require(malformed, "Invalid acquired image was not rejected.");

        using var cancellation = new CancellationTokenSource();
        var pixels = new byte[2048 * 1024];
        var rng = new Random(7313); rng.NextBytes(pixels);
        service.WatchFinish();
        var cancelled = McwQrDecoder.DecodeLuminanceAsync(2048, 1024, 2048, pixels, cancellation.Token);
        await service.FinishObserved.Task.WaitAsync(TimeSpan.FromSeconds(5));
        var timer = Stopwatch.StartNew(); cancellation.Cancel();
        var cancellationObserved = false;
        try { await cancelled; } catch (OperationCanceledException) { cancellationObserved = true; }
        timer.Stop();
        Require(cancellationObserved && timer.Elapsed < TimeSpan.FromSeconds(2), "Decode cancellation exceeded this fixture's observation limit.");
        service.CorruptNextFinish = true;
        var badReplyRejected = false;
        try { await McwQrDecoder.DecodeLuminanceAsync(32, 32, 32, Enumerable.Repeat((byte)255, 1024).ToArray()); }
        catch (InvalidDataException) { badReplyRejected = true; }
        Require(badReplyRejected, "Malformed Rust reply was accepted.");
        Require(await McwQrDecoder.DecodeLuminanceAsync(32, 32, 32, Enumerable.Repeat((byte)255, 1024).ToArray()) is null,
            "Abort/cancel poisoned subsequent requests.");
        Console.WriteLine(JsonSerializer.Serialize(new { production_caller_leaf_cases = tested, retained_repository_images = retained,
            multi_chunk = true, maximum_resolution = true,
            malformed_image_rejected = malformed, cancellation_verified = cancellationObserved, cancellation_ms = timer.Elapsed.TotalMilliseconds,
            cancellation_scope = "FINISH-dispatch marker synchronized, non-saturated Frame fixture; one request-return observation",
            diagnostic_redaction_verified = true,
            malformed_reply_rejected = badReplyRejected, capture_backend_replaced = false, camera_activated = false,
            shipping_host_dispatch_verified = false }));
    }
    private static async Task RunHosted(string[] args)
    {
        using var host = ManagedApplicationHost.Connect();
        var cases = JsonSerializer.Deserialize<List<Symbol>>(File.ReadAllText(args[0]))!;
        var tested = await CheckSymbols(cases);
        var retained = await CheckRetainedImages(args[2]);
        var pixels = new byte[4096 * 4096];
        new Random(7331).NextBytes(pixels);
        using var cancellation = new CancellationTokenSource();
        var pending = McwQrDecoder.DecodeLuminanceAsync(4096, 4096, 4096, pixels, cancellation.Token);
        // Cancel an incomplete upload. A decode that already finished before a
        // wall-clock delay is valid success, not ignored cancellation.
        Require(!pending.IsCompleted, "Synthetic upload completed before cancellation injection.");
        cancellation.Cancel();
        var cancelled = false;
        try { await pending; } catch (OperationCanceledException) { cancelled = true; }
        Require(cancelled, "Actual managed transport ignored cancellation.");
        Require(await McwQrDecoder.DecodeLuminanceAsync(32, 32, 32, Enumerable.Repeat((byte)255, 1024).ToArray()) is null,
            "Actual host transport failed after cancellation.");
        File.WriteAllText(args[1], JsonSerializer.Serialize(new { production_caller_leaf_cases = tested, retained_repository_images = retained,
            actual_managed_transport = true, patched_host_dispatch_verified = true, cancellation_verified = cancelled,
            cancellation_scope = "incomplete multi-chunk upload; immediate caller cancellation; no native FINISH checkpoint",
            diagnostic_redaction_verified = true,
            capture_backend_replaced = false, camera_activated = false, shipping_host_dispatch_verified = false }));
    }
    private static void VerifyDiagnosticRedaction()
    {
        const string marker = "MCW_QR_PRIVATE_MARKER_4e21";
        var payload = marker + "\0雪";
        var decoded = new QrDecodedText(payload, 40, 3, 17, 0, 2, 91);
        foreach (var diagnostic in new[] { decoded.ToString(), $"{(object)decoded}", (decoded with { Text = payload + "CLONE" }).ToString() })
        {
            Require(!diagnostic.Contains(marker, StringComparison.Ordinal) && diagnostic.Contains("<redacted>", StringComparison.Ordinal),
                "Decoded QR text appeared in an ordinary diagnostic.");
            Require(diagnostic.Contains("Version = 40", StringComparison.Ordinal), "Redaction discarded useful format metadata.");
        }
        Require(decoded.Text == payload, "Diagnostic redaction changed the decoded payload.");
    }
    private static async Task<int> CheckRetainedImages(string directory)
    {
        (string Name, string Text)[] expected = [
            ("AddressTest1.png", "tb1ql27ya3gufs5h0ptgjhjd0tm52fq6q0xrav7xza"),
            ("AddressTest2.png", "tb1qfas0k9rn8daqggu7wzp2yne9qdd5fr5wf2u478"),
            ("QrByPhone.jpg", "tb1qutgpgraaze3hqnvt2xyw5acsmd3urprk3ff27d"),
            ("QRwithZebraBackground.png", "Let's see a Zebra."),
            ("qr-embed-logos.png", "https://twitter.com/SimonHearne")
        ];
        foreach (var image in expected)
        {
            var actual = await DecodeCapturedImageAsync(new ArraySegment<byte>(File.ReadAllBytes(Path.Combine(directory, image.Name))), CancellationToken.None);
            Require(actual == image.Text, "The production image boundary changed a retained fixture result.");
        }
        return expected.Length;
    }
    private static async Task<int> CheckSymbols(List<Symbol> cases)
    {
        foreach (var item in cases)
        {
            var actual = await DecodeCapturedImageAsync(new ArraySegment<byte>(EncodeSymbol(item)), CancellationToken.None);
            Require(actual == item.expected, "The production capture decode leaf changed text.");
        }
        return cases.Count;
    }
    private static byte[] EncodeSymbol(Symbol item)
    {
        using (var bitmap = new SKBitmap((item.size + 8) * 3, (item.size + 8) * 3))
        {
            bitmap.Erase(SKColors.White);
            for (var y = 0; y < item.size; y++)
            for (var x = 0; x < item.size; x++)
            if (item.modules[y * item.size + x] == '1')
            {
                for (var dy = 0; dy < 3; dy++)
                for (var dx = 0; dx < 3; dx++) { bitmap.SetPixel((x + 4) * 3 + dx, (y + 4) * 3 + dy, SKColors.Black); }
            }
            using var encoded = bitmap.Encode(SKEncodedImageFormat.Png, 100);
            return encoded.ToArray();
        }
    }
    private static void Require(bool value, string message) { if (!value) { throw new Exception(message); } }
    public sealed record Symbol(int size, string modules, string expected);
}

sealed class FixtureConnection : IMcwApplicationServices, IAsyncDisposable
{
    private readonly Process _process;
    private readonly Stream _input;
    private readonly Stream _output;
    private readonly SemaphoreSlim _write = new(1);
    private readonly ConcurrentDictionary<ulong, (ushort Op, TaskCompletionSource<byte[]> Source)> _pending = new();
    private readonly CancellationTokenSource _stopped = new();
    private readonly Task _reader;
    private readonly Task _markers;
    private long _next;
    private long _finish;
    public bool CorruptNextFinish;
    public TaskCompletionSource<bool> FinishObserved { get; private set; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
    public CancellationToken Stopped => _stopped.Token;
    public FixtureConnection(string path)
    {
        _process = Process.Start(new ProcessStartInfo(path) { UseShellExecute = false, RedirectStandardInput = true,
            RedirectStandardOutput = true, RedirectStandardError = true, CreateNoWindow = true })!;
        _input = _process.StandardInput.BaseStream; _output = _process.StandardOutput.BaseStream;
        _reader = ReadLoop(); _markers = Markers();
    }
    public void WatchFinish() { Interlocked.Exchange(ref _finish, 0); FinishObserved = new(TaskCreationOptions.RunContinuationsAsynchronously); }
    public async Task Handshake()
    {
        var completion = new TaskCompletionSource<byte[]>(TaskCreationOptions.RunContinuationsAsynchronously);
        _pending.TryAdd(0, (0, completion)); await Write(1, 0, 0, ReadOnlyMemory<byte>.Empty);
        if ((await completion.Task.WaitAsync(TimeSpan.FromSeconds(5))).Length != 0) { throw new InvalidDataException("Handshake failed."); }
    }
    public async Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested(); Stopped.ThrowIfCancellationRequested();
        var id = (ulong)Interlocked.Increment(ref _next);
        var source = new TaskCompletionSource<byte[]>(TaskCreationOptions.RunContinuationsAsynchronously);
        if (!_pending.TryAdd(id, (operation, source))) { throw new IOException("Duplicate ID."); }
        if (operation == 0x1302) { Interlocked.Exchange(ref _finish, (long)id); }
        // Keep wire request ahead of its cancellation, as the production bridge does.
        await Write(3, id, operation, payload);
        using var registration = cancellationToken.Register(() =>
        {
            if (_pending.TryRemove(id, out var pending)) { pending.Source.TrySetCanceled(cancellationToken); _ = Cancel(id, operation); }
        });
        try { return await source.Task; }
        finally { _pending.TryRemove(id, out _); }
    }
    private async Task Cancel(ulong id, ushort op)
    {
        try { await Write(5, id, op, ReadOnlyMemory<byte>.Empty); } catch (IOException) { }
    }
    private async Task Write(byte kind, ulong id, ushort op, ReadOnlyMemory<byte> payload)
    {
        await _write.WaitAsync();
        try
        {
            var header = new byte[20]; BinaryPrimitives.WriteUInt32LittleEndian(header, (uint)(16 + payload.Length));
            BinaryPrimitives.WriteUInt16LittleEndian(header.AsSpan(4), 1); header[6] = kind;
            BinaryPrimitives.WriteUInt64LittleEndian(header.AsSpan(8), id); BinaryPrimitives.WriteUInt16LittleEndian(header.AsSpan(16), op);
            await _input.WriteAsync(header); await _input.WriteAsync(payload); await _input.FlushAsync();
        }
        finally { _write.Release(); }
    }
    private async Task ReadLoop()
    {
        try
        {
            while (!Stopped.IsCancellationRequested)
            {
                var prefix = new byte[4]; await _output.ReadExactlyAsync(prefix);
                var length = BinaryPrimitives.ReadUInt32LittleEndian(prefix);
                if (length is < 16 or > 1048576) { throw new InvalidDataException("Frame bound."); }
                var body = new byte[length]; await _output.ReadExactlyAsync(body);
                var id = BinaryPrimitives.ReadUInt64LittleEndian(body.AsSpan(4)); var op = BinaryPrimitives.ReadUInt16LittleEndian(body.AsSpan(12));
                if (!_pending.TryRemove(id, out var pending)) { continue; }
                if (pending.Op != op) { pending.Source.TrySetException(new InvalidDataException("Operation mismatch.")); continue; }
                if (body[2] == 4) { pending.Source.TrySetException(new IOException("Rust decode request rejected.")); continue; }
                var payload = body.AsSpan(16).ToArray();
                if (op == 0x1302 && CorruptNextFinish) { CorruptNextFinish = false; payload[0] = 2; }
                pending.Source.TrySetResult(payload);
            }
        }
        catch (Exception error) when (error is IOException or InvalidDataException)
        { foreach (var pending in _pending.Values) { pending.Source.TrySetException(error); } _stopped.Cancel(); }
    }
    private async Task Markers()
    {
        while (await _process.StandardError.ReadLineAsync() is { } line)
        {
            if (line.StartsWith("finish-start ", StringComparison.Ordinal) && long.TryParse(line.AsSpan(13), out var id)
                && id == Interlocked.Read(ref _finish)) { FinishObserved.TrySetResult(true); }
        }
    }
    public async ValueTask DisposeAsync()
    {
        _stopped.Cancel(); _input.Dispose();
        await _process.WaitForExitAsync().WaitAsync(TimeSpan.FromSeconds(5));
        await _reader; await _markers;
        if (_process.ExitCode != 0) { throw new Exception("Rust fixture did not exit cleanly."); }
        _process.Dispose(); _stopped.Dispose(); _write.Dispose();
    }
}
