// Own synthetic child only. Exact supplied shipping mcw drives its standard
// private-pipe protocol. No native hook, host patch, socket, wallet or UI.
using System;
using System.Buffers.Binary;
using System.IO;
using System.Runtime.InteropServices;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using Microsoft.Win32.SafeHandles;

internal static class ContentRawHost
{
    private const ushort Content = 0x0900;
    private const int PlainSize = 320 * 1024;
    private static void Check(bool value, string message)
    { if (!value) { throw new InvalidOperationException(message); } }

    public static async Task<int> Run(string[] arguments)
    {
        var mode = arguments[0][4..];
        Check(mode is "cancel" or "eof" or "saturation", "Unknown bounded content protocol mode");
        var spins = int.Parse(arguments[1], System.Globalization.CultureInfo.InvariantCulture);
        using var input = Console.OpenStandardInput(); using var output = Console.OpenStandardOutput();
        Console.SetOut(Console.Error);
        using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(20));
        var token = timeout.Token;
        await Write(output, 1, 0, 0, [], token).ConfigureAwait(false);
        var hello = await Read(input, token).ConfigureAwait(false);
        Check(hello.Kind == 2 && hello.Id == 0 && hello.Operation == 0, "Actual host handshake required");
        Array.Clear(hello.Payload);
        // Every REQUEST ID increases in wire order, exactly like the real writer.
        // The repeated CANCEL intentionally names its already admitted request.
        var body = new byte[PlainSize]; Array.Fill(body, (byte)42);
        for (var layer = 0; layer < 3; layer++) { body = StoredBrotli(body); }
        var packet = ContentHostFixture.NativePacket(body, "br, br, br");
        await Write(output, 3, 1, Content, packet, token).ConfigureAwait(false);
        Array.Clear(packet); Array.Clear(body);
        // This is an injection search, not proof of work. Only the native typed
        // partial-output counters below can establish in-flight interruption.
        Thread.SpinWait(spins);
        if (mode == "cancel") { await Write(output, 5, 1, Content, [], token).ConfigureAwait(false); }
        else if (mode == "eof") { CloseOutput(output); }
        else
        {
            using var batch = new MemoryStream();
            for (ulong id = 2; id <= 258; id++) { Encode(batch, 3, id, Content, []); }
            await output.WriteAsync(batch.ToArray(), token).ConfigureAwait(false);
            await output.FlushAsync(token).ConfigureAwait(false);
        }
        var partial = false; var queueLimit = false; var shutdown = false;
        ulong consumed = 0, produced = 0; byte failedLayer = 0;
        for (var frames = 0; frames < 300; frames++)
        {
            var response = await Read(input, token).ConfigureAwait(false);
            try
            {
                if (response.Id == 1 && response.Operation == Content && response.Kind == 2)
                {
                    var bytes = response.Payload;
                    if (bytes.Length == 22 && bytes[0] == 1 && bytes[1] == 0 && bytes[2] == 1
                        && BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(3)) == 5)
                    {
                        failedLayer = bytes[5];
                        consumed = BinaryPrimitives.ReadUInt64LittleEndian(bytes.AsSpan(6));
                        produced = BinaryPrimitives.ReadUInt64LittleEndian(bytes.AsSpan(14));
                        partial = consumed > 0 && produced > 0 && produced < PlainSize && failedLayer < 3;
                    }
                    // No success body or startup-only cancellation can pass.
                    if (mode == "cancel") { break; }
                    // A completed/startup-only saturation trial cannot prove
                    // active overload. Finish this miss rather than waiting for
                    // a shutdown that an unsaturated queue need never produce.
                    if (mode == "saturation" && !partial) { break; }
                }
                if (response.Kind == 4 && response.Id == 258 && response.Operation == Content)
                { queueLimit = response.Payload.Length >= 2 && BinaryPrimitives.ReadUInt16LittleEndian(response.Payload) == 4; }
                if (response.Kind == 3 && response.Id == 0 && response.Operation == 2 && response.Payload.Length == 0)
                { shutdown = true; break; }
            }
            finally { Array.Clear(response.Payload); }
        }
        if (mode == "cancel")
        {
            // The live connection and a later sibling must remain usable.
            var follow = ContentHostFixture.NativePacket(Encoding.ASCII.GetBytes("synthetic sibling"));
            await Write(output, 3, 2, Content, follow, token).ConfigureAwait(false);
            var sibling = await Read(input, token).ConfigureAwait(false);
            Check(sibling.Id == 2 && sibling.Kind == 2 && sibling.Payload.Length == 29
                && sibling.Payload[2] == 0 && sibling.Payload[11] == 0, "Post-cancel sibling must survive");
            Array.Clear(sibling.Payload);
            await Write(output, 3, 3, 2, [], token).ConfigureAwait(false);
            var stopped = await Read(input, token).ConfigureAwait(false);
            Check(stopped.Kind == 2 && stopped.Id == 3 && stopped.Operation == 2 && stopped.Payload.Length == 0, "Orderly host shutdown acknowledgement");
            shutdown = true;
        }
        var accepted = partial && shutdown && (mode != "saturation" || queueLimit);
        Console.Error.WriteLine($"CONTENT RAW {(accepted ? "VERIFIED" : "NONPARTIAL")} mode={mode} input={consumed} output={produced} layer={failedLayer} full={PlainSize} queue_limit={queueLimit.ToString().ToLowerInvariant()} shutdown={shutdown.ToString().ToLowerInvariant()} monotonically_increasing_requests=true");
        return accepted ? 0 : 75;
    }

    private sealed record Frame(byte Kind, ulong Id, ushort Operation, byte[] Payload);
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GetStdHandle(int handle);
    private static void CloseOutput(Stream output)
    {
        // Console streams need not own their OS descriptor. Close this child's
        // real pipe endpoint so native EOF occurs while its input stays open.
        output.Dispose();
        using var descriptor = new SafeFileHandle(OperatingSystem.IsWindows() ? GetStdHandle(-11) : new IntPtr(1), true);
        Check(!descriptor.IsInvalid, "Owned stdout descriptor required for EOF");
    }
    private static async Task<Frame> Read(Stream input, CancellationToken token)
    {
        var prefix = new byte[4]; await input.ReadExactlyAsync(prefix, token).ConfigureAwait(false);
        var length = BinaryPrimitives.ReadInt32LittleEndian(prefix);
        Check(length is >= 16 and <= 1048576, "Bounded native frame");
        var bytes = new byte[length]; await input.ReadExactlyAsync(bytes, token).ConfigureAwait(false);
        Check(BinaryPrimitives.ReadUInt16LittleEndian(bytes) == 1 && bytes[3] == 0 && bytes[14] == 0 && bytes[15] == 0, "Native frame header");
        var result = new Frame(bytes[2], BinaryPrimitives.ReadUInt64LittleEndian(bytes.AsSpan(4)), BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(12)), bytes[16..]);
        Array.Clear(bytes); return result;
    }
    private static void Encode(Stream output, byte kind, ulong id, ushort operation, byte[] payload)
    {
        using var writer = new BinaryWriter(output, Encoding.UTF8, true);
        writer.Write(16 + payload.Length); writer.Write((ushort)1); writer.Write(kind); writer.Write((byte)0);
        writer.Write(id); writer.Write(operation); writer.Write((ushort)0); writer.Write(payload);
    }
    private static async Task Write(Stream output, byte kind, ulong id, ushort operation, byte[] payload, CancellationToken token)
    {
        using var frame = new MemoryStream(); Encode(frame, kind, id, operation, payload);
        var bytes = frame.ToArray();
        try { await output.WriteAsync(bytes, token).ConfigureAwait(false); await output.FlushAsync(token).ConfigureAwait(false); }
        finally { Array.Clear(bytes); }
    }
    private sealed class Bits
    {
        private readonly MemoryStream _bytes = new(); private uint _value; private int _count;
        public void Raw(uint value, int count)
        {
            for (var i = 0; i < count; i++)
            {
                _value |= ((value >> i) & 1) << _count;
                if (++_count == 8) { _bytes.WriteByte((byte)_value); _value = 0; _count = 0; }
            }
        }
        public void Align() { if (_count != 0) { Raw(0, 8 - _count); } }
        public byte[] Finish() { Align(); return _bytes.ToArray(); }
    }
    private static byte[] StoredBrotli(byte[] plain)
    {
        var bits = new Bits(); bits.Raw(0, 1); // RFC7932 WBITS=16
        for (var offset = 0; offset < plain.Length; offset += 65536)
        {
            var count = Math.Min(65536, plain.Length - offset);
            bits.Raw(0, 1); bits.Raw(0, 2); bits.Raw((uint)count - 1, 16); bits.Raw(1, 1); bits.Align();
            foreach (var value in plain.AsSpan(offset, count)) { bits.Raw(value, 8); }
        }
        bits.Raw(3, 2); return bits.Finish();
    }
}
