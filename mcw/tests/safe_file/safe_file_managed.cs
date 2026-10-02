using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.Diagnostics;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Client.Application;

// Development-only test connection: real Rust native service, independent frame
// transport. This is not a production connection, host, executable or fallback.
public sealed class NativeConnection : IMcwApplicationServices, IDisposable
{
    private readonly Process _process;
    private readonly Stream _input, _output;
    private ulong _id;
    public int Requests { get; private set; }
    public CancellationToken Stopped => CancellationToken.None;
    public NativeConnection(string executable)
    {
        var start = new ProcessStartInfo(executable) { RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true, UseShellExecute = false, CreateNoWindow = true };
        _process = Process.Start(start)!; _input = _process.StandardInput.BaseStream; _output = _process.StandardOutput.BaseStream;
    }
    public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        ulong id = ++_id; Requests++;
        byte[] frame = new byte[20 + payload.Length];
        BinaryPrimitives.WriteUInt32LittleEndian(frame, (uint)(16 + payload.Length));
        BinaryPrimitives.WriteUInt16LittleEndian(frame.AsSpan(4), 1); frame[6] = 3;
        BinaryPrimitives.WriteUInt64LittleEndian(frame.AsSpan(8), id);
        BinaryPrimitives.WriteUInt16LittleEndian(frame.AsSpan(16), operation);
        payload.CopyTo(frame.AsMemory(20)); _input.Write(frame); _input.Flush();
        byte[] prefix = new byte[4]; _output.ReadExactly(prefix);
        int length = checked((int)BinaryPrimitives.ReadUInt32LittleEndian(prefix));
        if (length < 16 || length > 1_048_576) { throw new IOException("bad test frame size"); }
        byte[] response = new byte[length]; _output.ReadExactly(response);
        if (response[2] != 2 || BinaryPrimitives.ReadUInt64LittleEndian(response.AsSpan(4)) != id || BinaryPrimitives.ReadUInt16LittleEndian(response.AsSpan(12)) != operation) { throw new IOException("bad test frame identity"); }
        return Task.FromResult(response[16..]);
    }
    public void Dispose()
    {
        _input.Dispose();
        if (!_process.WaitForExit(5000)) { throw new IOException("native test service did not exit"); }
        if (_process.ExitCode != 0) { throw new IOException(_process.StandardError.ReadToEnd()); }
        _output.Dispose(); _process.Dispose();
    }
}
public static class Program
{
    static string Snapshot(string path) => string.Join("|", new[] { "", ".new", ".old" }.Select(s => File.Exists(path + s) ? Convert.ToHexString(File.ReadAllBytes(path + s)) : Directory.Exists(path + s) ? "DIRECTORY" : "MISSING"));
    static void Seed(string path, int mask)
    {
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        if ((mask & 1) != 0) { File.WriteAllBytes(path, Encoding.UTF8.GetBytes("old-complete")); }
        if ((mask & 2) != 0) { File.WriteAllBytes(path + ".new", Encoding.UTF8.GetBytes("stale-new")); }
        if ((mask & 4) != 0) { File.WriteAllBytes(path + ".old", Encoding.UTF8.GetBytes("older-backup")); }
    }
    static string? Try(Action call) { try { call(); return null; } catch (Exception e) { return $"{e.GetType().Name}:{(e as ArgumentException)?.ParamName}:{e.HResult:X8}"; } }
    public static int Main(string[] args)
    {
        string root = Path.GetFullPath(args[1]); Directory.CreateDirectory(root);
        using var application = Environment.GetEnvironmentVariable("MCW_HOSTED") == "1" ? ManagedApplicationHost.Connect() : null;
        using var connection = application is null ? new NativeConnection(args[0]) : null;
        using var binding = connection is not null ? McwApplicationServices.Bind(connection) : null;
        var evidence = new List<object>(); int cases = 0;
        var encodings = new (string, Encoding)[] { ("utf8bom", Encoding.UTF8), ("utf8", new UTF8Encoding(false)), ("utf16le", Encoding.Unicode), ("utf16be", Encoding.BigEndianUnicode), ("utf32", Encoding.UTF32), ("ascii", Encoding.ASCII) };
        foreach (var (name, encoding) in encodings)
        foreach (string text in new[] { "", "synthetic 🧙 العربية\0\r\n", new string('x', 1_100_013) })
        foreach (int mask in Enumerable.Range(0, 8))
        {
            string directory = Path.Combine(root, "case-" + cases++); string old = Path.Combine(directory, "oracle", "synthetic.wallet"); string candidate = Path.Combine(directory, "rust", "synthetic.wallet");
            Seed(old, mask); Seed(candidate, mask);
            string? expectedError = Try(() => OracleInvoker.Text(old, text, encoding));
            string? actualError = Try(() => CandidateInvoker.Text(candidate, text, encoding));
            if (expectedError != actualError || Snapshot(old) != Snapshot(candidate)) { throw new Exception($"safe-file parity failed: {name}/{text.Length}/{mask}: {expectedError}/{actualError}"); }
            evidence.Add(new { encoding = name, length = text.Length, initial_mask = mask, matched = true });
        }
        foreach (int mask in Enumerable.Range(0, 8))
        foreach (byte[] bytes in new[] { Array.Empty<byte>(), Enumerable.Range(0, 256).Select(x => (byte)x).ToArray(), Enumerable.Range(0, 1_200_019).Select(x => (byte)(x % 251)).ToArray() })
        {
            string directory = Path.Combine(root, "case-" + cases++); string old = Path.Combine(directory, "oracle", "synthetic.wallet"); string candidate = Path.Combine(directory, "rust", "synthetic.wallet"); Seed(old, mask); Seed(candidate, mask);
            string? expectedError = Try(() => OracleInvoker.Bytes(old, bytes)); string? actualError = Try(() => CandidateInvoker.Bytes(candidate, bytes));
            if (expectedError != actualError || Snapshot(old) != Snapshot(candidate)) { throw new Exception("safe-file binary parity failed"); }
            evidence.Add(new { bytes = bytes.Length, initial_mask = mask, matched = true });
        }
        var encodingCases = new (string name, string? text, Encoding? encoding)[]
        {
            ("short-invalid-utf8", "x\ud800", new UTF8Encoding(true, true)),
            ("large-invalid-utf8", new string('x', 8191) + "\ud800", new UTF8Encoding(true, true)),
            ("short-invalid-ascii", "🧙", Encoding.GetEncoding("us-ascii", EncoderFallback.ExceptionFallback, DecoderFallback.ExceptionFallback)),
            ("large-invalid-ascii", new string('x', 8192) + "🧙", Encoding.GetEncoding("us-ascii", EncoderFallback.ExceptionFallback, DecoderFallback.ExceptionFallback)),
            ("split-surrogate", new string('x', 8191) + "🧙end", new UTF8Encoding(true, true)),
            ("null-text", null, Encoding.UTF8),
            ("null-encoding", "synthetic", null)
        };
        foreach (var item in encodingCases)
        foreach (int mask in Enumerable.Range(0, 8))
        {
            string directory = Path.Combine(root, "encoding-" + cases++); string old = Path.Combine(directory, "oracle", "synthetic.wallet"); string candidate = Path.Combine(directory, "rust", "synthetic.wallet");
            Seed(old, mask); Seed(candidate, mask);
            string? expectedError = Try(() => OracleInvoker.Text(old, item.text!, item.encoding!));
            string? actualError = Try(() => CandidateInvoker.Text(candidate, item.text!, item.encoding!));
            if (expectedError != actualError || Snapshot(old) != Snapshot(candidate)) { throw new Exception($"safe-file encoding-fault parity failed: {item.name}/{mask} {expectedError}/{actualError}"); }
            evidence.Add(new { encoding_case = item.name, initial_mask = mask, expectedError, actualError, matched = true });
        }
        foreach (string invalid in new[] { "null-encoding", "null-bytes" })
        {
            string directory = Path.Combine(root, "validation-" + cases++); string old = Path.Combine(directory, "oracle", "nested", "synthetic.wallet"); string candidate = Path.Combine(directory, "rust", "nested", "synthetic.wallet");
            string? expectedError = Try(() => { if (invalid == "null-bytes") { OracleInvoker.Bytes(old, null!); } else { OracleInvoker.Text(old, "synthetic", null!); } });
            string? actualError = Try(() => { if (invalid == "null-bytes") { CandidateInvoker.Bytes(candidate, null!); } else { CandidateInvoker.Text(candidate, "synthetic", null!); } });
            if (expectedError != actualError || Snapshot(old) != Snapshot(candidate) || Directory.Exists(Path.GetDirectoryName(old)) != Directory.Exists(Path.GetDirectoryName(candidate))) { throw new Exception("safe-file directory/validation order parity failed"); }
            evidence.Add(new { invalid, expectedError, actualError, matched = true });
        }
        foreach (string shape in new[] { "unicode-and-spaces", "dot-component", "trailing-separator" })
        {
            string directory = Path.Combine(root, "paths-" + cases++); string old = Path.Combine(directory, "oracle", "name 🧙 with spaces"); string candidate = Path.Combine(directory, "rust", "name 🧙 with spaces");
            Directory.CreateDirectory(old); Directory.CreateDirectory(candidate);
            if (shape == "dot-component") { old += "/."; candidate += "/."; }
            else if (shape == "trailing-separator") { old += "/"; candidate += "/"; }
            else { old += "/synthetic.wallet"; candidate += "/synthetic.wallet"; }
            string? expectedError = Try(() => OracleInvoker.Text(old, "synthetic", Encoding.UTF8)); string? actualError = Try(() => CandidateInvoker.Text(candidate, "synthetic", Encoding.UTF8));
            if (expectedError != actualError || Snapshot(old) != Snapshot(candidate)) { throw new Exception($"safe-file path parity failed: {shape} {expectedError}/{actualError}"); }
            evidence.Add(new { shape, expectedError, actualError, matched = true });
        }
        foreach (string fault in new[] { "new-directory", "old-directory", "parent-file", "new-readonly", "old-readonly", "main-readonly" })
        {
            string directory = Path.Combine(root, "fault-" + fault); string old = Path.Combine(directory, "oracle", "synthetic.wallet"); string candidate = Path.Combine(directory, "rust", "synthetic.wallet");
            foreach (string path in new[] { old, candidate })
            {
                Seed(path, 1);
                if (fault == "new-directory") { Directory.CreateDirectory(path + ".new"); }
                if (fault == "old-directory") { Directory.CreateDirectory(path + ".old"); }
                if (fault.EndsWith("readonly")) { string selected = fault == "new-readonly" ? path + ".new" : fault == "old-readonly" ? path + ".old" : path; if (!File.Exists(selected)) { File.WriteAllBytes(selected, Encoding.UTF8.GetBytes("readonly-content")); } File.SetAttributes(selected, FileAttributes.ReadOnly); }
                if (fault == "parent-file") { File.WriteAllBytes(path + "-parent-file", new byte[] { 1 }); }
            }
            if (fault == "parent-file") { old += "-parent-file/child"; candidate += "-parent-file/child"; }
            string? expectedError = Try(() => OracleInvoker.Bytes(old, new byte[] { 2, 3, 4 })); string? actualError = Try(() => CandidateInvoker.Bytes(candidate, new byte[] { 2, 3, 4 }));
            if (expectedError != actualError || Snapshot(old) != Snapshot(candidate)) { throw new Exception($"safe-file fault parity failed: {fault} {expectedError}/{actualError}"); }
            evidence.Add(new { fault, expectedError, actualError, matched = true }); cases++;
        }
        var result = new { cases, requests = connection?.Requests, passed = true, actual_application_bridge = application is not null, reference = "original SafeFile.cs helper using .NET 10, development-only" };
        File.WriteAllText(Path.Combine(root, "managed-reference.json"), JsonSerializer.Serialize(new { result, evidence })); Console.WriteLine(JsonSerializer.Serialize(result)); return 0;
    }
}
