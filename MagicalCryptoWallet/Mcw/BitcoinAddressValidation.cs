using System;
using System.Buffers.Binary;
using System.Diagnostics.CodeAnalysis;
using System.IO;
using System.Text;
using NBitcoin;

namespace MagicalCryptoWallet.Mcw;

/// <summary>Typed adapter for Rust address validation. No managed text-decoder fallback.</summary>
public static class BitcoinAddressValidation
{
    public const ushort Operation = 0x020c;
    private static readonly UTF8Encoding Utf8 = new(false, true);

    public static bool TryGetScriptPubKey(string? address, NBitcoin.Network network, [NotNullWhen(true)] out byte[]? scriptPubKey)
    {
        scriptPubKey = null;
        if (address is null) { return false; }
        byte networkId;
        switch (network.Name)
        {
            case "Main": networkId = 0; break;
            case "TestNet": networkId = 1; break;
            case "TestNet4": networkId = 2; break;
            case "signet": networkId = 3; break;
            case "RegTest": networkId = 4; break;
            default: return false;
        }
        int length;
        try { length = Utf8.GetByteCount(address); }
        catch (EncoderFallbackException) { return false; }
        if (length > 90) { return false; }
        var payload = new byte[length + 1];
        payload[0] = networkId;
        Utf8.GetBytes(address, payload.AsSpan(1));
        var service = McwApplicationServices.Current;
        var result = service.RequestAsync(Operation, payload, service.Stopped).GetAwaiter().GetResult();
        if (result.Length == 3 && result[0] == 0)
        {
            var reason = BinaryPrimitives.ReadUInt16LittleEndian(result.AsSpan(1));
            if (reason is >= 1 and <= 5) { return false; }
        }
        if (result.Length < 2 || result[0] != 1 || !IsStandardScript(result.AsSpan(1)))
        { throw new IOException("Invalid mcw address validation response."); }
        scriptPubKey = result[1..];
        return true;
    }

    private static bool IsStandardScript(ReadOnlySpan<byte> script)
    {
        if (script.Length == 25 && script[0] == 0x76 && script[1] == 0xa9 && script[2] == 20
            && script[23] == 0x88 && script[24] == 0xac) { return true; }
        if (script.Length == 23 && script[0] == 0xa9 && script[1] == 20 && script[22] == 0x87) { return true; }
        if (script.Length is < 4 or > 42 || script[1] != script.Length - 2) { return false; }
        return script[0] == 0 ? script[1] is 20 or 32 : script[0] is >= 0x51 and <= 0x60;
    }
}
