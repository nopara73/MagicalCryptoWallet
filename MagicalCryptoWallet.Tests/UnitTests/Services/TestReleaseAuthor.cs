using NBitcoin.Secp256k1;
using System.Threading.Tasks;
using NNostr.Client;
using NNostr.Client.Protocols;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Tests.UnitTests.Services;

// Deliberately public synthetic key. It is never trusted by the application.
internal static class TestReleaseAuthor
{
	private const string Secret = "0000000000000000000000000000000000000000000000000000000000000001";
	public static ECXOnlyPubKey PublicKey { get; } = CreatePublicKey();
	public static string Npub { get; } = PublicKey.ToNIP19();

	private static ECXOnlyPubKey CreatePublicKey()
	{
		using var key = NostrExtensions.ParseKey(Secret);
		return key.CreateXOnlyPubKey();
	}

	public static async Task<NostrEvent> CreateReleaseAsync(Version version, string? signingSecret = null,
		Action<NostrEvent>? beforeSigning = null)
	{
		var note = new NostrEvent
		{
			Kind = 1, CreatedAt = DateTimeOffset.UtcNow, Content = "Synthetic release verification",
			Tags = [new() { TagIdentifier = "version", Data = [version.ToString()] }]
		};
		foreach (var name in new[] { "SHA256SUMS", "SHA256SUMS.asc", "SHA256SUMS.magicalcryptowalletsig", $"MagicalCryptoWallet-{version}.msi" })
		{
			note.Tags.Add(new() { TagIdentifier = name, Data = [$"{Constants.RepositoryUrl}/releases/download/v{version}/{name}"] });
		}
		beforeSigning?.Invoke(note);
		using var key = NostrExtensions.ParseKey(signingSecret ?? Secret);
		return await note.ComputeIdAndSignAsync(key);
	}
}
