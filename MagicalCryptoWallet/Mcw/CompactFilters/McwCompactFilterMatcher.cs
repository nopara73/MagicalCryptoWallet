using System;
using System.Buffers.Binary;
using System.IO;
using System.Threading;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Mcw.CompactFilters;

/// <summary>Typed temporary adapter to first-party basic-filter matching in mcw.</summary>
public static class McwCompactFilterMatcher
{
    public const ushort MatchAnyOperation = 0x0702;
    private const int MaxPayload = 1024 * 1024 - 16;
    private const int MaxQueries = 65_536;

    public static async Task<bool> MatchAnyAsync(byte[] encodedFilter, byte[] blockHash,
        byte[][] scripts, CancellationToken cancellationToken = default)
    {
        ArgumentNullException.ThrowIfNull(encodedFilter);
        ArgumentNullException.ThrowIfNull(blockHash);
        ArgumentNullException.ThrowIfNull(scripts);
        cancellationToken.ThrowIfCancellationRequested();
        if (blockHash.Length != 32) { throw new ArgumentException("Block hash must contain 32 raw wire-order bytes.", nameof(blockHash)); }
        if (scripts.Length > MaxQueries) { throw new IOException("Too many compact-filter queries."); }
        long length = 32L + 4 + encodedFilter.Length + 4;
        foreach (var script in scripts)
        {
            if (script is null) { throw new ArgumentException("Query scripts cannot be null.", nameof(scripts)); }
            length += 4L + script.Length;
            if (length > MaxPayload) { throw new IOException("Compact-filter request exceeds the application frame limit."); }
        }
        if (length > MaxPayload) { throw new IOException("Compact-filter request exceeds the application frame limit."); }
        var payload = new byte[(int)length];
        blockHash.CopyTo(payload, 0);
        BinaryPrimitives.WriteUInt32LittleEndian(payload.AsSpan(32), (uint)encodedFilter.Length);
        encodedFilter.CopyTo(payload, 36);
        var offset = 36 + encodedFilter.Length;
        BinaryPrimitives.WriteUInt32LittleEndian(payload.AsSpan(offset), (uint)scripts.Length);
        offset += 4;
        foreach (var script in scripts)
        {
            BinaryPrimitives.WriteUInt32LittleEndian(payload.AsSpan(offset), (uint)script.Length);
            offset += 4;
            script.CopyTo(payload, offset);
            offset += script.Length;
        }
        cancellationToken.ThrowIfCancellationRequested();
        var result = await McwApplicationServices.Current.RequestAsync(MatchAnyOperation, payload,
            cancellationToken).ConfigureAwait(false);
        if (result.Length != 1 || result[0] > 1) { throw new IOException("Invalid compact-filter match response."); }
        return result[0] == 1;
    }
}
