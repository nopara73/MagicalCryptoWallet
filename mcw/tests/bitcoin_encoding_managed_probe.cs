// Test-only child of the real mcw host. Never included in a release package.
using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Userfacing;
using NBitcoin;

internal static class BitcoinAddressProbe
{
    private const string ZeroHashAddress = "1111111111111111111114oLvT2";
    private static int _assertions;

    private static void Check(bool condition, string message)
    {
        if (!condition) { throw new InvalidOperationException(message); }
        _assertions++;
    }

    private static void Throws<T>(Action action, string message) where T : Exception
    {
        try { action(); }
        catch (T) { _assertions++; return; }
        throw new InvalidOperationException(message);
    }

    private static bool Reference(string address, Network network, out BitcoinAddress? parsed)
    {
        // Retained package is an independent compatibility oracle only in tests.
        try { parsed = Network.Parse<BitcoinAddress>(address, network); return true; }
        catch { parsed = null; return false; }
    }

    private static Network NetworkFor(string value) => value switch
    {
        "main" => Network.Main,
        "test" or "testnet4" or "signet" => Network.TestNet4,
        "regtest" => Network.RegTest,
        _ => throw new InvalidOperationException("Unknown public fixture network.")
    };

    private static void CheckCompatibility(string text, Network network, byte[] script, ref int accepted, ref int unsupported)
    {
        Check(BitcoinAddressValidation.TryGetScriptPubKey(text, network, out var bytes), "Native valid address rejected.");
        Check(bytes!.SequenceEqual(script), "Native script differs from the primary fixture.");
        var expected = Reference(text, network, out var reference);
        var actual = NBitcoinExtensions.TryParseBitcoinAddressForNetwork(text, network, out var address);
        Check(actual == expected, "Retained address-object policy changed for " + text);
        if (actual)
        {
            Check(address!.ScriptPubKey.ToBytes().SequenceEqual(script), "Managed address changed script bytes.");
            Check(address.ToString() == reference!.ToString(), "Managed address changed canonical text.");
            accepted++;
        }
        else { Check(address is null, "Unsupported managed address has a value."); unsupported++; }
    }

    private sealed class ReplyService(byte[]? reply, Exception? error = null) : IMcwApplicationServices
    {
        public CancellationToken Stopped => CancellationToken.None;
        public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
        {
            Check(operation == BitcoinAddressValidation.Operation, "Wrong production operation.");
            Check(payload.Span[0] == 0 && Encoding.UTF8.GetString(payload.Span[1..]) == ZeroHashAddress, "Caller changed address bytes.");
            return error is null ? Task.FromResult(reply!) : Task.FromException<byte[]>(error);
        }
    }

    private static void CheckFailClosedAdapter()
    {
        Throws<InvalidOperationException>(() => NBitcoinExtensions.TryParseBitcoinAddressForNetwork(ZeroHashAddress, Network.Main, out _), "Unbound caller used a managed fallback.");
        foreach (var response in new byte[][] { [], [0, 0, 0], [0, 6, 0], [0, 1], [1], [1, 0, 20], [2, 1, 0] })
        {
            using var binding = McwApplicationServices.Bind(new ReplyService(response));
            Throws<IOException>(() => NBitcoinExtensions.TryParseBitcoinAddressForNetwork(ZeroHashAddress, Network.Main, out _), "Malformed reply was accepted.");
        }
        using (McwApplicationServices.Bind(new ReplyService(null, new IOException("synthetic bridge failure"))))
        {
            Throws<IOException>(() => AddressParser.ParseBitcoinAddress(ZeroHashAddress, Network.Main), "Transport failure became invalid-address or fallback success.");
        }
        using (McwApplicationServices.Bind(new ReplyService([0, 3, 0])))
        {
            Check(!NBitcoinExtensions.TryParseBitcoinAddressForNetwork(ZeroHashAddress, Network.Main, out var address) && address is null, "Typed address rejection was lost.");
        }
    }

    public static async Task<int> Main(string[] args)
    {
        try
        {
            if (args.Length != 2) { throw new InvalidOperationException("Expected fixture directory and report path."); }
            CheckFailClosedAdapter();
            using var host = ManagedApplicationHost.Connect();
            using var stopped = new CancellationTokenSource();
            host.BindShutdown(stopped.Cancel);
            var service = McwApplicationServices.Current;
            Check(ReferenceEquals(service, host), "Production service connection was not bound.");
            var accepted = 0;
            var unsupported = 0;
            var core = 0;
            foreach (var line in File.ReadLines(Path.Combine(args[0], "core_addresses.tsv")).Where(line => !line.StartsWith('#')))
            {
                var fields = line.Split('\t');
                var network = NetworkFor(fields[0]);
                var script = Convert.FromHexString(fields[2]);
                var explicitNetwork = fields[0] switch { "main" => 0, "test" => 1, "testnet4" => 2, "signet" => 3, "regtest" => 4, _ => throw new InvalidOperationException("Unknown network.") };
                var payload = new byte[1 + Encoding.UTF8.GetByteCount(fields[1])];
                payload[0] = (byte)explicitNetwork;
                Encoding.UTF8.GetBytes(fields[1], payload.AsSpan(1));
                var native = await service.RequestAsync(BitcoinAddressValidation.Operation, payload);
                Check(native.Length == script.Length + 1 && native[0] == 1 && native.AsSpan(1).SequenceEqual(script), "Explicit native network mapping changed.");
                CheckCompatibility(fields[1], network, script, ref accepted, ref unsupported);
                if (fields[1].StartsWith("bc1", StringComparison.OrdinalIgnoreCase) || fields[1].StartsWith("tb1", StringComparison.OrdinalIgnoreCase) || fields[1].StartsWith("bcrt1", StringComparison.OrdinalIgnoreCase))
                { CheckCompatibility(fields[1].ToUpperInvariant(), network, script, ref accepted, ref unsupported); }
                core++;
            }
            Check(core == 54, "Primary valid fixture count changed.");
            var invalid = 0;
            foreach (var line in File.ReadLines(Path.Combine(args[0], "core_invalid_addresses.tsv")).Where(line => !line.StartsWith('#')))
            {
                var text = new UTF8Encoding(false, true).GetString(Convert.FromHexString(line));
                foreach (var network in new[] { Network.Main, Network.TestNet4, Network.RegTest })
                {
                    Check(!BitcoinAddressValidation.TryGetScriptPubKey(text, network, out _), "Primary invalid address accepted.");
                    Check(!NBitcoinExtensions.TryParseBitcoinAddressForNetwork(text, network, out var address) && address is null, "Invalid caller has an address value.");
                    Check(!Reference(text, network, out _), "Retained parser accepts a primary invalid fixture.");
                }
                invalid++;
            }
            Check(invalid == 70, "Primary invalid fixture count changed.");
            Check(!NBitcoinExtensions.TryParseBitcoinAddressForNetwork(" " + ZeroHashAddress, Network.Main, out _), "Common helper silently trimmed.");
            Check(!NBitcoinExtensions.TryParseBitcoinAddressForNetwork(ZeroHashAddress, Network.TestNet4, out _), "Wrong network accepted.");
            Check(!NBitcoinExtensions.TryParseBitcoinAddressForNetwork("\ud800", Network.Main, out _), "Invalid UTF-16 was repaired.");
            Check(!NBitcoinExtensions.TryParseBitcoinAddressForNetwork(new string('1', 91), Network.Main, out _), "Oversize text was sent.");
            var lexicalCases = new[] { " " + ZeroHashAddress, ZeroHashAddress + " ", "\t" + ZeroHashAddress, ZeroHashAddress + "\n", "\0" + ZeroHashAddress, ZeroHashAddress + "\0", "", "  " + ZeroHashAddress + "  " };
            foreach (var text in lexicalCases)
            {
                Check(NBitcoinExtensions.TryParseBitcoinAddressForNetwork(text, Network.Main, out _) == Reference(text, Network.Main, out _), "Common-helper lexical compatibility differs: " + Convert.ToHexString(Encoding.UTF8.GetBytes(text)));
            }
            var trimmed = AddressParser.Parse("  " + ZeroHashAddress + " \n", Network.Main);
            Check(trimmed.IsOk && trimmed.Value.ToCanonicalAddress(Network.Main) == ZeroHashAddress, "Existing UI trim contract changed.");
            var uri = AddressParser.Parse("bitcoin:" + ZeroHashAddress + "?amount=0.0001&label=Exact%20Text", Network.Main);
            Check(uri.IsOk && uri.Value is MagicalCryptoWallet.Userfacing.Address.Bip21Uri { Amount: 0.0001m, Label: "Exact Text" }, "Actual BIP21 address caller changed.");
            Check(!AddressParser.Parse("bitcoin:" + ZeroHashAddress + "?req-unknown=x", Network.Main).IsOk, "Required URI parameter policy changed.");

            var malformed = 0;
            foreach (var payload in new byte[][] { [], [255], [0, 0xff] })
            {
                try { await service.RequestAsync(BitcoinAddressValidation.Operation, payload); throw new InvalidOperationException("Malformed transport payload accepted."); }
                catch (IOException) { malformed++; }
                Check(BitcoinAddressValidation.TryGetScriptPubKey(ZeroHashAddress, Network.Main, out _), "Persistent bridge did not recover.");
            }
            try { await service.RequestAsync(ushort.MaxValue, ReadOnlyMemory<byte>.Empty); throw new InvalidOperationException("Unknown operation accepted."); }
            catch (IOException) { malformed++; }
            using (var canceled = new CancellationTokenSource())
            {
                canceled.Cancel();
                try { await service.RequestAsync(BitcoinAddressValidation.Operation, new byte[] { 0 }, canceled.Token); throw new InvalidOperationException("Canceled request ran."); }
                catch (OperationCanceledException) { _assertions++; }
            }
            var requests = Enumerable.Range(0, 64).Select(async _ =>
            {
                var payload = new byte[1 + ZeroHashAddress.Length];
                Encoding.UTF8.GetBytes(ZeroHashAddress, payload.AsSpan(1));
                var result = await service.RequestAsync(BitcoinAddressValidation.Operation, payload);
                if (result.Length != 26 || result[0] != 1 || result[1] != 0x76 || result[^1] != 0xac)
                { throw new InvalidOperationException("Concurrent caller reply changed."); }
            });
            await Task.WhenAll(requests);
            Check(!host.Stopped.IsCancellationRequested && !stopped.IsCancellationRequested, "Service channel stopped during requests.");
            File.WriteAllText(args[1], JsonSerializer.Serialize(new
            {
                core_valid = core, core_invalid = invalid, explicit_native_network_cases = core, managed_valid_cases = accepted,
                retained_unsupported_cases = unsupported, malformed_requests = malformed,
                concurrent_requests = 64, assertions = _assertions,
                lexical_compatibility_cases = lexicalCases.Length,
                networks = new[] { Network.Main.Name, Network.TestNet.Name, Network.TestNet4.Name, Network.RegTest.Name },
                actual_caller = "NBitcoinExtensions.TryParseBitcoinAddressForNetwork / AddressParser",
                actual_transport = "ManagedApplicationHost / mcw app dispatch",
                dependency_removed = false
            }, new JsonSerializerOptions { WriteIndented = true }));
            Console.Error.WriteLine("Address integration probe passed.");
            return 0;
        }
        catch (Exception error) { Console.Error.WriteLine(error); return 1; }
    }
}
