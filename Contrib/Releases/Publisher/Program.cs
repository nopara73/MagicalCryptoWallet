using System.Security.Cryptography;
using System.Text.Json;
using NBitcoin;
using NBitcoin.Crypto;
using NNostr.Client;
using NNostr.Client.Protocols;

namespace MagicalCryptoWallet.ReleaseTools;

internal static class Program
{
	private const string SignatureKeyVariable = "MAGICALCRYPTOWALLET_UPDATE_SIGNING_KEY";
	private const string AnnouncementKeyVariable = "MAGICALCRYPTOWALLET_NOSTR_ANNOUNCEMENT_KEY";
	private const string Repository = "https://github.com/nopara73/MagicalCryptoWallet";

	private static async Task<int> Main(string[] args)
	{
		try
		{
			switch (args)
			{
				case ["sign-manifest", var manifest, var signature]:
					using (var key = Key.Parse(RequiredSecret(SignatureKeyVariable), Network.Main))
					{
						if (key.PubKey.ToHex() != TrustPin("update_public_key")) throw new InvalidOperationException("Wrong update key.");
						var digest = new uint256(SHA256.HashData(await File.ReadAllBytesAsync(manifest)));
						await File.WriteAllTextAsync(signature, Convert.ToBase64String(key.Sign(digest).ToDER()) + "\n");
					}
					return 0;
				case ["verify-manifest", var manifest, var signature, var publicKey]:
					var hash = new uint256(SHA256.HashData(await File.ReadAllBytesAsync(manifest)));
					var proof = ECDSASignature.FromDER(Convert.FromBase64String(await File.ReadAllTextAsync(signature)));
					return new PubKey(publicKey).Verify(hash, proof) ? 0 : 1;
				case ["prepare-announcement", var version, var contentFile, var packages]:
					if (!Version.TryParse(version, out var parsedVersion) || parsedVersion.ToString() != version)
						throw new InvalidOperationException("Invalid release version.");
					var files = Directory.GetFiles(packages).Select(Path.GetFileName).Order(StringComparer.Ordinal).ToArray();
					foreach (var required in new[] { "SHA256SUMS", "SHA256SUMS.asc", "SHA256SUMS.magicalcryptowalletsig" })
						if (!files.Contains(required)) throw new InvalidOperationException("Sign the manifest before preparing an announcement.");
					var note = new NostrEvent
					{
						Kind = 1, CreatedAt = DateTimeOffset.UtcNow,
						Content = await File.ReadAllTextAsync(contentFile),
						Tags = [new() { TagIdentifier = "version", Data = [version] }]
					};
					foreach (var file in files.Where(f => f != "release-announcement.json"))
						note.Tags.Add(new() { TagIdentifier = file!, Data = [$"{Repository}/releases/download/v{version}/{file}"] });
					using (var key = NostrExtensions.ParseKey(RequiredSecret(AnnouncementKeyVariable)))
					{
						if (key.CreateXOnlyPubKey().ToHex() != TrustPin("announcement_public_key")) throw new InvalidOperationException("Wrong announcement key.");
						note = await note.ComputeIdAndSignAsync(key);
					}
					if (!note.Verify() || note.ComputeId() != note.Id) throw new InvalidOperationException("Announcement verification failed.");
					await File.WriteAllTextAsync(Path.Combine(packages, "release-announcement.json"), JsonSerializer.Serialize(note));
					return 0;
				default:
					Console.Error.WriteLine("Usage: sign-manifest <manifest> <signature> | verify-manifest <manifest> <signature> <public-key> | prepare-announcement <version> <content-file> <packages-directory>");
					return 2;
			}
		}
		catch
		{
			// Do not echo key material from parsing exceptions or command arguments.
			Console.Error.WriteLine("Release signing or verification failed. Check the inputs and configured signing environment.");
			return 1;
		}
	}

	private static string RequiredSecret(string variable) =>
		Environment.GetEnvironmentVariable(variable) is { Length: > 0 } value
			? value : throw new InvalidOperationException("Missing signing secret.");

	private static string TrustPin(string name)
	{
		using var stream = typeof(Program).Assembly.GetManifestResourceStream("MagicalCryptoWallet.ReleaseKeys.json")!;
		using var document = JsonDocument.Parse(stream);
		return document.RootElement.GetProperty(name).GetString()!;
	}
}
