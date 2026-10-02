using System.Collections.Generic;
using System.Threading;
using System.Threading.Tasks;
using NNostr.Client;
using MagicalCryptoWallet.Tests.UnitTests.Services;
using MagicalCryptoWallet.WebClients;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.WebClients;

public class MagicalCryptoWalletNostrClientTests
{
    [Theory]
    [InlineData("valid", true)]
    [InlineData("wrong-author", false)]
    [InlineData("forged-author", false)]
    [InlineData("tampered-content", false)]
    [InlineData("missing-manifest", false)]
    [InlineData("wrong-destination", false)]
    [InlineData("duplicate-version", false)]
    [InlineData("wrong-kind", false)]
    public async Task AuthenticatesReleaseAnnouncementsAsync(string scenario, bool accepted)
    {
        using var transport = new TesteabletNostrClient([], manualMode: true);
        using var client = new MagicalCryptoWalletNostrClient(transport, TestReleaseAuthor.Npub);
        await client.ConnectAndSubscribeAsync(CancellationToken.None);
        var note = await TestReleaseAuthor.CreateReleaseAsync(new Version(2, 5, 0),
            signingSecret: scenario is "wrong-author" or "forged-author"
                ? "0000000000000000000000000000000000000000000000000000000000000002" : null,
            beforeSigning: n =>
            {
                switch (scenario)
                {
                    case "missing-manifest": n.Tags.RemoveAll(t => t.TagIdentifier == "SHA256SUMS"); break;
                    case "wrong-destination": n.Tags[1].Data = ["https://example.invalid/SHA256SUMS"]; break;
                    case "duplicate-version": n.Tags.Add(new() { TagIdentifier = "version", Data = ["99.0.0"] }); break;
                    case "wrong-kind": n.Kind = 2; break;
                }
            });
        if (scenario == "forged-author") note.PublicKey = TestReleaseAuthor.PublicKey.ToHex();
        if (scenario == "tampered-content") note.Content += "tampered";
        transport.SimulateEventsReceived([note]);
        transport.SimulateEoseReceived();
        var releases = new List<ReleaseInfo>();
        await foreach (var release in client.EventsReader.ReadAllAsync()) releases.Add(release);
        Assert.Equal(accepted ? 1 : 0, releases.Count);
        if (accepted) Assert.Equal(new Version(2, 5, 0), releases[0].Version);
    }
}
