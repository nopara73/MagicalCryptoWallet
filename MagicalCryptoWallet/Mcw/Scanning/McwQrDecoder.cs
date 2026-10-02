using System;
using System.Buffers.Binary;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Mcw.Scanning;

public sealed record QrDecodedText(string Text, byte Version, byte ErrorCorrectionLevel, ushort CorrectedSymbols,
    byte? StructuredIndex, byte? StructuredTotal, byte? StructuredParity)
{
    public override string ToString() =>
        $"QrDecodedText {{ Text = <redacted>, Version = {Version}, ErrorCorrectionLevel = {ErrorCorrectionLevel}, CorrectedSymbols = {CorrectedSymbols}, StructuredIndex = {StructuredIndex}, StructuredTotal = {StructuredTotal}, StructuredParity = {StructuredParity} }}";
}

/// <summary>Temporary pixel adapter. The sole QR decoding implementation is mcw.</summary>
public static class McwQrDecoder
{
    private const ushort Begin = 0x1300;
    private const ushort Append = 0x1301;
    private const ushort Finish = 0x1302;
    private const ushort Abort = 0x1303;
    private const int ChunkSize = 262144;
    private const int MaxBytes = 16777216;
    private static readonly UTF8Encoding StrictUtf8 = new(false, true);
    private static long _nextTransfer;

    public static async Task<QrDecodedText?> DecodeLuminanceAsync(uint width, uint height, uint stride,
        ReadOnlyMemory<byte> luminance, CancellationToken cancellationToken = default)
    {
        if (width is 0 or > 4096 || height is 0 or > 4096 || stride < width || stride > 16384
            || (ulong)width * height > MaxBytes || (ulong)stride * height != (ulong)luminance.Length
            || luminance.Length > MaxBytes)
        { throw new ArgumentException("QR luminance frame exceeds its bounds.", nameof(luminance)); }
        cancellationToken.ThrowIfCancellationRequested();
        var service = McwApplicationServices.Current;
        using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, service.Stopped);
        var token = linked.Token;
        var transfer = Interlocked.Increment(ref _nextTransfer);
        if (transfer <= 0) { throw new InvalidOperationException("The QR transfer ID limit was reached."); }
        var completed = false;
        try
        {
            var begin = Header((ulong)transfer, 24);
            BinaryPrimitives.WriteUInt32LittleEndian(begin.AsSpan(12), width);
            BinaryPrimitives.WriteUInt32LittleEndian(begin.AsSpan(16), height);
            BinaryPrimitives.WriteUInt32LittleEndian(begin.AsSpan(20), stride);
            CheckAck(await service.RequestAsync(Begin, begin, token).ConfigureAwait(false));
            for (var offset = 0; offset < luminance.Length; offset += ChunkSize)
            {
                token.ThrowIfCancellationRequested();
                var length = Math.Min(ChunkSize, luminance.Length - offset);
                var chunk = Header((ulong)transfer, 16 + length);
                BinaryPrimitives.WriteUInt32LittleEndian(chunk.AsSpan(12), (uint)offset);
                luminance.Slice(offset, length).Span.CopyTo(chunk.AsSpan(16));
                CheckAck(await service.RequestAsync(Append, chunk, token).ConfigureAwait(false));
            }
            var response = await service.RequestAsync(Finish, Header((ulong)transfer, 12), token).ConfigureAwait(false);
            var decoded = DecodeResponse(response);
            token.ThrowIfCancellationRequested();
            completed = true;
            return decoded;
        }
        finally
        {
            if (!completed && !service.Stopped.IsCancellationRequested)
            {
                // The ticket is client-assigned, so even a cancelled BEGIN with
                // a drained late reply has an explicit, idempotent cleanup path.
                using var cleanup = CancellationTokenSource.CreateLinkedTokenSource(service.Stopped);
                cleanup.CancelAfter(TimeSpan.FromSeconds(1));
                try { CheckAck(await service.RequestAsync(Abort, Header((ulong)transfer, 12), cleanup.Token).ConfigureAwait(false)); }
                catch (Exception error) when (error is OperationCanceledException or IOException or InvalidOperationException)
                { /* Rust owner Drop/10-second expiry is the remaining cleanup boundary. */ }
            }
        }
    }

    private static byte[] Header(ulong transfer, int length)
    {
        var bytes = new byte[length];
        BinaryPrimitives.WriteUInt16LittleEndian(bytes, 1);
        BinaryPrimitives.WriteUInt64LittleEndian(bytes.AsSpan(4), transfer);
        return bytes;
    }
    private static void CheckAck(byte[] bytes)
    {
        if (!bytes.AsSpan().SequenceEqual(new byte[] { 1, 0, 0, 0 })) { throw InvalidResponse(); }
    }
    private static InvalidDataException InvalidResponse() => new("Invalid mcw QR decode response.");

    private static QrDecodedText? DecodeResponse(byte[] bytes)
    {
        if (bytes.Length < 4 || BinaryPrimitives.ReadUInt16LittleEndian(bytes) != 1 || bytes[3] != 0) { throw InvalidResponse(); }
        if (bytes[2] == 0) { if (bytes.Length != 4) { throw InvalidResponse(); } return null; }
        if (bytes[2] != 1 || bytes.Length < 16 || bytes[4] is 0 or > 40 || bytes[5] > 3) { throw InvalidResponse(); }
        var corrected = BinaryPrimitives.ReadUInt16LittleEndian(bytes.AsSpan(6));
        var length = BinaryPrimitives.ReadUInt32LittleEndian(bytes.AsSpan(12));
        if (corrected > 1215 || length > 32768 || length != bytes.Length - 16) { throw InvalidResponse(); }
        if (bytes[11] == 0)
        { if (bytes[8] != 0 || bytes[9] != 0 || bytes[10] != 0) { throw InvalidResponse(); } }
        else if (bytes[11] != 1 || bytes[9] is < 2 or > 16 || bytes[8] >= bytes[9]) { throw InvalidResponse(); }
        string text;
        try { text = StrictUtf8.GetString(bytes, 16, (int)length); }
        catch (DecoderFallbackException) { throw InvalidResponse(); }
        return new QrDecodedText(text, bytes[4], bytes[5], corrected,
            bytes[11] == 1 ? bytes[8] : null, bytes[11] == 1 ? bytes[9] : null, bytes[11] == 1 ? bytes[10] : null);
    }
}
