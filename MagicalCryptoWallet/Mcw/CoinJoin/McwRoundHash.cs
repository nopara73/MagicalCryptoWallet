using System;
using System.Collections.Immutable;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.WabiSabi.Models;

namespace MagicalCryptoWallet.Mcw.CoinJoin;

/// <summary>Client-only round fingerprint adapter; coordinator hashing remains independent.</summary>
public static class McwRoundHash
{
    public const ushort Operation = 0x1200;
    public const ushort PayloadVersion = 1;
    private const int MaximumPayload = 1_048_560;
    private const int MaximumString = 65_536;
    private const int MaximumScriptTypes = 64;

    public static async Task<uint256> CalculateAsync(RoundState round, CancellationToken cancellationToken = default)
    {
        cancellationToken.ThrowIfCancellationRequested();
        var payload = Encode(round);
        var result = await McwApplicationServices.Current.RequestAsync(Operation, payload, cancellationToken).ConfigureAwait(false);
        if (result.Length != 32) { throw new IOException("Invalid mcw round fingerprint response."); }
        // NBitcoin uint256(byte[]) and the coordinator's reference both use
        // these raw bytes. No display-order hex reversal belongs in this pipe.
        return new uint256(result);
    }

    public static byte[] Encode(RoundState round)
    {
        ArgumentNullException.ThrowIfNull(round);
        var parameters = round.CoinjoinState.Parameters;
        using var buffer = new MemoryStream();
        using var writer = new BinaryWriter(buffer, Encoding.UTF8, leaveOpen: true);
        writer.Write(PayloadVersion);
        writer.Write((ushort)0);
        writer.Write(round.InputRegistrationStart.ToUnixTimeMilliseconds());
        writer.Write(round.InputRegistrationTimeout.Ticks);
        writer.Write(parameters.ConnectionConfirmationTimeout.Ticks);
        writer.Write(parameters.OutputRegistrationTimeout.Ticks);
        writer.Write(parameters.TransactionSigningTimeout.Ticks);
        writer.Write(parameters.AllowedInputAmounts.Min.Satoshi);
        writer.Write(parameters.AllowedInputAmounts.Max.Satoshi);
        WriteScripts(writer, parameters.AllowedInputTypes);
        writer.Write(parameters.AllowedOutputAmounts.Min.Satoshi);
        writer.Write(parameters.AllowedOutputAmounts.Max.Satoshi);
        WriteScripts(writer, parameters.AllowedOutputTypes);
        WriteString(writer, parameters.Network.ToString());
        writer.Write(parameters.MiningFeeRate.FeePerK.Satoshi);
        writer.Write(parameters.MaxTransactionSize);
        writer.Write(parameters.MinRelayTxFee.FeePerK.Satoshi);
        writer.Write(parameters.MaxAmountCredentialValue.Satoshi);
        writer.Write((long)parameters.MaxVsizeCredentialValue);
        writer.Write((long)parameters.MaxVsizeAllocationPerAlice);
        writer.Write(parameters.MaxSuggestedAmount.Satoshi);
        WriteString(writer, parameters.CoordinationIdentifier);
        writer.Write(round.AmountCredentialIssuerParameters.Cw.ToBytes());
        writer.Write(round.AmountCredentialIssuerParameters.I.ToBytes());
        writer.Write(round.VsizeCredentialIssuerParameters.Cw.ToBytes());
        writer.Write(round.VsizeCredentialIssuerParameters.I.ToBytes());
        writer.Flush();
        if (buffer.Length > MaximumPayload) { throw new InvalidDataException("Round fingerprint payload exceeds the service bound."); }
        return buffer.ToArray();
    }

    private static void WriteScripts(BinaryWriter writer, ImmutableSortedSet<ScriptType> scriptTypes)
    {
        if (scriptTypes.Count > MaximumScriptTypes) { throw new InvalidDataException("Too many round script types."); }
        writer.Write((ushort)scriptTypes.Count);
        // Preserve the existing set's iteration order, including a custom
        // comparer. The reference labels the sequence by index in this order.
        foreach (var scriptType in scriptTypes)
        {
            WriteString(writer, Enum.GetName(scriptType) ?? throw new NotSupportedException("Unknown round script type."));
        }
    }

    private static void WriteString(BinaryWriter writer, string value)
    {
        var count = Encoding.UTF8.GetByteCount(value);
        if (count > MaximumString) { throw new InvalidDataException("Round fingerprint string exceeds the service bound."); }
        writer.Write((uint)count);
        writer.Write(Encoding.UTF8.GetBytes(value));
    }
}
