using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Runtime.InteropServices;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw;
using ZXing;
using ZXing.Common;
using ZXing.QrCode;

// Test-only managed child. The package never contains this program. It exercises
// the production adapter through the production Rust host with synthetic data.
// Redirect ordinary logging before opening the private protocol streams.
Console.SetOut(TextWriter.Null);
Console.SetOut(Console.Error);
if (args.Length == 0) { return 2; }
var action = args[0];
var report = args.Length > 1 ? Path.GetFullPath(args[1]) : "";
if (action == "early-exit") { return 0; }
if (action == "no-handshake") { await Task.Delay(TimeSpan.FromSeconds(180)); return 0; }
if (action.StartsWith("sessions-", StringComparison.Ordinal)) { return await SessionProbe.Run(action, report); }
if (action is "queue-eof" or "queue-overload") { return await ProbeQueueAsync(action, report); }
if (action is "bad-version" or "bad-length" or "truncated")
{
    var frame = new byte[20]; frame[0] = 16; frame[4] = 2; frame[6] = 1;
    if (action == "bad-length") { frame[0] = 1; frame[2] = 16; }
    if (action == "truncated") { frame = [16]; }
    using var output = Console.OpenStandardOutput();
    await output.WriteAsync(frame);
    await Task.Delay(1000);
    return 0;
}
using var host = ManagedApplicationHost.Connect();
if (action == "unexpected-exit") { Environment.Exit(0); }
using var stop = new CancellationTokenSource();
host.BindShutdown(stop.Cancel);
if (host.StartupArguments.Length != 0) { args = host.StartupArguments; action = args[0]; report = args[1]; }
if (action == "wait")
{
    File.WriteAllText(report, Environment.ProcessId.ToString());
    try { await Task.Delay(Timeout.Infinite, stop.Token); }
    catch (OperationCanceledException) { File.AppendAllText(report, "\nshutdown"); }
    return 0;
}
if (action == "unknown-operation")
{
    try { await McwApplicationServices.Current.RequestAsync(ushort.MaxValue, new byte[] { 0, 1, 2 }); throw new Exception("Unknown operation was accepted."); }
    catch (IOException) { }
    Decode(await host.GenerateQrAsync("AFTER UNKNOWN OPERATION"), "AFTER UNKNOWN OPERATION");
    return 0;
}
if (action == "restart")
{
    host.Handoff(ManagedApplicationHost.RestartOperation, ["exit-7", report, "space path / 你好"]);
    return 0;
}
if (action == "crash")
{
    host.Handoff(ManagedApplicationHost.CrashOperation, ["exit-7", report, "private synthetic exception"]);
    return 1;
}
if (action == "update")
{
    // A synthetic, nonexistent MSI must be rejected without launching an installer.
    host.Handoff(ManagedApplicationHost.UpdateOperation, [report + ".msi"]);
    return 0;
}
if (action == "exit-7")
{
    File.WriteAllText(report, JsonSerializer.Serialize(new { arguments = args, processArguments = Environment.GetCommandLineArgs() }));
    return 7;
}
if (action == "qr")
{
    var count = 0;
    var vectors = new[] { "01234567890123456789", "HELLO WORLD $%*+-./:", "Mixed Case  \n\t",
        "你好 café Ελληνικά", "🦀😀🌍", "bitcoin:bc1qsynthetic?amount=0.001&label=Exact%20Text", "\uFEFFexplicit BOM" };
    // Multiple requests share one pipe and may complete out of order.
    await Task.WhenAll(from level in Enumerable.Range(0, 4)
                       from text in vectors
                       select CheckAsync(text, (byte)level));
    count += vectors.Length * 4;
    // Independently decode all 160 version/ECC boundaries. These are the
    // standard's byte-mode capacities, not expected encoder matrices.
    int[][] capacities =
    [
        [17,32,53,78,106,134,154,192,230,271,321,367,425,458,520,586,644,718,792,858,929,1003,1091,1171,1273,1367,1465,1528,1628,1732,1840,1952,2068,2188,2303,2431,2563,2699,2809,2953],
        [14,26,42,62,84,106,122,152,180,213,251,287,331,362,412,450,504,560,624,666,711,779,857,911,997,1059,1125,1190,1264,1370,1452,1538,1628,1722,1809,1911,1989,2099,2213,2331],
        [11,20,32,46,60,74,86,108,130,151,177,203,241,258,292,322,364,394,442,482,509,565,611,661,715,751,805,868,908,982,1030,1112,1168,1228,1283,1351,1423,1499,1579,1663],
        [7,14,24,34,44,58,64,84,98,119,137,155,177,194,220,250,280,310,338,382,403,439,461,511,535,593,625,658,698,742,790,842,898,958,983,1051,1093,1139,1219,1273]
    ];
    for (byte level = 0; level < 4; level++)
    {
        for (var version = 1; version <= 40; version++)
        {
            var text = new string('a', capacities[level][version - 1]);
            var matrix = await host.GenerateQrAsync(text, level);
            if (matrix.GetLength(0) != version * 4 + 17) { throw new Exception("Incorrect smallest QR version."); }
            Decode(matrix, text, pure: true);
            count++;
        }
    }
    foreach (var pair in new[] { (0,7089),(1,5596),(2,3993),(3,3057) })
    {
        Decode(await host.GenerateQrAsync(new string('1', pair.Item2), (byte)pair.Item1), new string('1', pair.Item2), pure: true);
        count++;
    }
    await ExpectErrorAsync(() => host.GenerateQrAsync(""));
    await ExpectErrorAsync(() => host.GenerateQrAsync(new string('a', 2954), 0));
    await ExpectErrorAsync(() => host.GenerateQrAsync("text", 4));
    using (var cancellation = new CancellationTokenSource())
    {
        var pending = host.GenerateQrAsync(new string('a', 2331), cancellationToken: cancellation.Token);
        cancellation.Cancel();
        try { await pending; throw new Exception("Cancellation was ignored."); }
        catch (OperationCanceledException) { }
        // Late replies have been drained and the next operation still succeeds.
        await CheckAsync("AFTER CANCELLATION", 1);
    }
    File.WriteAllText(report, JsonSerializer.Serialize(new { decoded = count, canceled = true }));
    return 0;

    async Task CheckAsync(string text, byte level) => Decode(await host.GenerateQrAsync(text, level), text);
}
return 2;

static async Task<int> ProbeQueueAsync(string action, string report)
{
    using var input = Console.OpenStandardInput();
    using var output = Console.OpenStandardOutput();
    await WriteFrameAsync(output, 1, 0, 0, []);
    var hello = await ReadFrameAsync(input);
    if (hello.Kind != 2 || hello.Id != 0 || hello.Operation != 0) { throw new IOException("Invalid queue-probe handshake."); }
    // Read replies concurrently so host output backpressure cannot cause the
    // backlog. Thousands of maximum-version symbols saturate request ingress.
    var reader = Task.Run(async () =>
    {
        var replies = 0;
        var overloaded = false;
        while (true)
        {
            var frame = await ReadFrameAsync(input);
            if (frame.Kind == 3 && frame.Id == 0 && frame.Operation == 2)
            { return (Replies: replies, Overloaded: overloaded, Shutdown: true); }
            if (frame.Kind == 4 && frame.Payload.Length >= 2 && BinaryPrimitives.ReadUInt16LittleEndian(frame.Payload) == 4)
            { overloaded = true; }
            else if (frame.Kind == 2 && frame.Operation == 1) { replies++; }
            else { throw new IOException("Unexpected queue-probe response."); }
        }
    });
    var payload = new byte[7090];
    Array.Fill(payload, (byte)'1');
    payload[0] = 0;
    var count = action == "queue-eof" ? 200 : 4096;
    try
    {
        for (var id = 1; id <= count && !reader.IsCompleted; id++)
        { await WriteFrameAsync(output, 3, (ulong)id, 1, payload); }
        if (action == "queue-eof") { await WriteFrameAsync(output, 5, 199, 1, []); }
    }
    catch (IOException) when (action == "queue-overload")
    { /* Saturation closes the child's write pipe and explicitly requests cleanup. */ }
    output.Dispose();
    if (action == "queue-eof") { ProbePipe.CloseOutput(); }
    var result = await reader.WaitAsync(TimeSpan.FromSeconds(15));
    if (action == "queue-overload" && !result.Overloaded) { throw new IOException("No typed queue-overload error."); }
    if (action == "queue-eof" && result.Replies >= count) { throw new IOException("EOF did not discard backlogged work."); }
    File.WriteAllText(report, JsonSerializer.Serialize(new { result.Replies, result.Overloaded, result.Shutdown }));
    return 0;
}

static async Task WriteFrameAsync(Stream output, byte kind, ulong id, ushort operation, byte[] payload)
{
    var frame = new byte[20 + payload.Length];
    BinaryPrimitives.WriteInt32LittleEndian(frame, frame.Length - 4);
    BinaryPrimitives.WriteUInt16LittleEndian(frame.AsSpan(4), 1);
    frame[6] = kind;
    BinaryPrimitives.WriteUInt64LittleEndian(frame.AsSpan(8), id);
    BinaryPrimitives.WriteUInt16LittleEndian(frame.AsSpan(16), operation);
    payload.CopyTo(frame, 20);
    await output.WriteAsync(frame);
    await output.FlushAsync();
}

static async Task<(byte Kind, ulong Id, ushort Operation, byte[] Payload)> ReadFrameAsync(Stream input)
{
    var prefix = new byte[4];
    await input.ReadExactlyAsync(prefix);
    var length = BinaryPrimitives.ReadInt32LittleEndian(prefix);
    if (length is < 16 or > 1024 * 1024) { throw new IOException("Invalid queue-probe frame length."); }
    var frame = new byte[length];
    await input.ReadExactlyAsync(frame);
    if (BinaryPrimitives.ReadUInt16LittleEndian(frame) != 1 || frame[3] != 0 || frame[14] != 0 || frame[15] != 0)
    { throw new IOException("Invalid queue-probe frame header."); }
    return (frame[2], BinaryPrimitives.ReadUInt64LittleEndian(frame.AsSpan(4)), BinaryPrimitives.ReadUInt16LittleEndian(frame.AsSpan(12)), frame[16..]);
}

static async Task ExpectErrorAsync(Func<Task<bool[,]>> action)
{
    try { await action(); throw new Exception("Invalid QR request was accepted."); }
    catch (IOException) { }
}
static void Decode(bool[,] matrix, string expected, bool pure = false)
{
    const int scale = 4;
    var width = matrix.GetLength(0);
    var size = (width + 8) * scale;
    var pixels = Enumerable.Repeat((byte)255, size * size * 3).ToArray();
    for (var y = 0; y < size; y++)
    {
        for (var x = 0; x < size; x++)
        {
            var (mx, my) = (x / scale - 4, y / scale - 4);
            if (mx >= 0 && my >= 0 && mx < width && my < width && matrix[mx, my])
            {
                var offset = (y * size + x) * 3;
                pixels[offset] = pixels[offset + 1] = pixels[offset + 2] = 0;
            }
        }
    }
    var source = new RGBLuminanceSource(pixels, size, size, RGBLuminanceSource.BitmapFormat.RGB24);
    // Dense, homogeneous capacity vectors exercise the independent symbol
    // decoder directly. Ordinary content/UI checks also exercise image detection.
    var hints = new Dictionary<DecodeHintType, object>();
    if (pure) { hints[DecodeHintType.PURE_BARCODE] = true; }
    var result = new QRCodeReader().decode(new BinaryBitmap(new HybridBinarizer(source)), hints);
    if (result?.Text != expected) { throw new Exception($"Independent decoder mismatch for synthetic vector: width {width}, input length {expected.Length}, expected {Convert.ToHexString(Encoding.UTF8.GetBytes(expected[..Math.Min(40,expected.Length)]))}, actual {Convert.ToHexString(Encoding.UTF8.GetBytes(result?.Text ?? "<null>"))}."); }
}
