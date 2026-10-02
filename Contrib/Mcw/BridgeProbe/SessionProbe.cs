using System;
using System.Buffers.Binary;
using System.IO;
using System.Text;
using System.Text.Json;
using System.Threading.Tasks;

// Synthetic owned sessions through the actual subprocess/private pipe. This
// proves resource cleanup and terminal lifecycle, not an in-progress checkpoint.
internal static class SessionProbe
{
    private const ulong Reader = 777, Ticket = 888;
    private sealed record Frame(byte Kind, ulong Id, ushort Op, byte[] Body);
    private static void Check(bool value) { if (!value) { throw new IOException("Synthetic session cleanup check failed."); } }

    public static async Task<int> Run(string action, string report)
    {
        using var input = Console.OpenStandardInput();
        using var output = Console.OpenStandardOutput();
        var file = report + ".wallet";
        File.WriteAllText(file, "original synthetic bytes");
        await Write(output, 1, 0, 0, []);
        Check((await Read(input)).Kind == 2);
        var path = Encoding.UTF8.GetBytes(file);
        var begin = new byte[23 + path.Length];
        BinaryPrimitives.WriteUInt16LittleEndian(begin, 1);
        BinaryPrimitives.WriteUInt64LittleEndian(begin.AsSpan(2), 1);
        BinaryPrimitives.WriteUInt32LittleEndian(begin.AsSpan(19), (uint)path.Length);
        path.CopyTo(begin, 23);
        await Write(output, 3, 1, 0x1000, begin);
        var opened = await Read(input);
        Check(opened.Kind == 2 && opened.Id == 1 && opened.Body.Length == 11 && opened.Body[2] == 0);
        var session = BinaryPrimitives.ReadUInt64LittleEndian(opened.Body.AsSpan(3));
        var tor = new byte[9]; BinaryPrimitives.WriteUInt64LittleEndian(tor, Reader);
        await Write(output, 3, 2, 0x0F02, tor);
        Check((await Read(input)).Kind == 2);
        var scan = new byte[24];
        BinaryPrimitives.WriteUInt16LittleEndian(scan, 1);
        BinaryPrimitives.WriteUInt64LittleEndian(scan.AsSpan(4), Ticket);
        foreach (var offset in new[] { 12, 16, 20 }) { BinaryPrimitives.WriteUInt32LittleEndian(scan.AsSpan(offset), 21); }
        await Write(output, 3, 3, 0x1300, scan);
        Check((await Read(input)).Kind == 2);
        if (action == "sessions-cancel")
        {
            await Write(output, 5, 1, 0x1000, []);
            await Write(output, 5, 2, 0x0F02, []);
            await Write(output, 5, 3, 0x1300, []);
            var feed = new byte[10]; BinaryPrimitives.WriteUInt64LittleEndian(feed, Reader);
            feed[9] = (byte)'x';
            await Write(output, 3, 4, 0x0F03, feed);
            Check((await Read(input)).Kind == 4);
            var appendScan = new byte[17]; scan.AsSpan(0, 12).CopyTo(appendScan);
            await Write(output, 3, 5, 0x1301, appendScan);
            Check((await Read(input)).Kind == 4);
            var append = new byte[19];
            BinaryPrimitives.WriteUInt16LittleEndian(append, 1);
            BinaryPrimitives.WriteUInt64LittleEndian(append.AsSpan(2), session);
            append[18] = 42;
            await Write(output, 3, 6, 0x1001, append);
            var failed = await Read(input);
            Check(failed.Kind == 2 && failed.Body.Length == 9 && failed.Body[2] == 1);
            await Write(output, 3, 7, 1, [1, (byte)'x']);
            Check((await Read(input)).Kind == 2);
            CheckFiles(file);
            await Write(output, 3, 8, 2, []);
            Check((await Read(input)).Id == 8);
        }
        else
        {
            Task? writer = null;
            if (action == "sessions-overload")
            {
                writer = Task.Run(async () =>
                {
                    var payload = new byte[7090]; Array.Fill(payload, (byte)'1'); payload[0] = 0;
                    try
                    {
                        for (ulong id = 4; id <= 4100; id++) { await Write(output, 3, id, 1, payload); }
                    }
                    catch (IOException) { }
                });
            }
            else if (action == "sessions-eof") { output.Dispose(); ProbePipe.CloseOutput(); }
            else { Check(action == "sessions-wait"); File.WriteAllText(report, Environment.ProcessId.ToString()); }
            var overloaded = false;
            while (true)
            {
                var frame = await Read(input);
                if (frame.Kind == 4 && frame.Body.Length >= 2 && BinaryPrimitives.ReadUInt16LittleEndian(frame.Body) == 4) { overloaded = true; }
                if (frame.Kind == 3 && frame.Id == 0 && frame.Op == 2) { break; }
            }
            if (writer is not null) { await writer.WaitAsync(TimeSpan.FromSeconds(15)); Check(overloaded); }
            CheckFiles(file);
            if (action == "sessions-wait")
            {
                await Write(output, 3, 4, 2, []);
                Check((await Read(input)).Id == 4);
                File.AppendAllText(report, "\nshutdown");
                return 0;
            }
        }
        File.WriteAllText(report, JsonSerializer.Serialize(new { action, cleanup = true, file, child_pid = Environment.ProcessId,
            recovery_artifact_preserved = true, private_pipe_subprocess = true, checkpoint_proof = false }));
        return 0;
    }

    private static void CheckFiles(string file)
    {
        Check(File.ReadAllText(file) == "original synthetic bytes" && !File.Exists(file + ".old"));
        // Exclusive native/managed locking interoperation verifies the host's
        // .new stream is closed; the recovery artifact itself must survive.
        using var reopened = new FileStream(file + ".new", FileMode.Open, FileAccess.ReadWrite, FileShare.None);
        Check(reopened.Length == 0);
    }
    private static async Task<Frame> Read(Stream input)
    {
        var prefix = new byte[4]; await input.ReadExactlyAsync(prefix);
        var length = BinaryPrimitives.ReadInt32LittleEndian(prefix);
        Check(length is >= 16 and <= 1048576);
        var bytes = new byte[length]; await input.ReadExactlyAsync(bytes);
        Check(BinaryPrimitives.ReadUInt16LittleEndian(bytes) == 1 && bytes[3] == 0 && bytes[14] == 0 && bytes[15] == 0);
        return new(bytes[2], BinaryPrimitives.ReadUInt64LittleEndian(bytes.AsSpan(4)), BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(12)), bytes[16..]);
    }
    private static async Task Write(Stream output, byte kind, ulong id, ushort op, byte[] body)
    {
        var bytes = new byte[20 + body.Length];
        BinaryPrimitives.WriteInt32LittleEndian(bytes, bytes.Length - 4);
        BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(4), 1);
        bytes[6] = kind;
        BinaryPrimitives.WriteUInt64LittleEndian(bytes.AsSpan(8), id);
        BinaryPrimitives.WriteUInt16LittleEndian(bytes.AsSpan(16), op);
        body.CopyTo(bytes, 20);
        await output.WriteAsync(bytes); await output.FlushAsync();
    }
}
