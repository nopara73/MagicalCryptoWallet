using System.IO;
using System.Threading;
using System.Threading.Tasks;
using System.Security.Cryptography;
using NBitcoin;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Services;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Services;

public class UpdateSignatureTests
{
	[Fact]
	public async Task AcceptsValidSignatureAndRejectsTamperingAndWrongKeyAsync()
	{
		var directory = Path.Combine(Path.GetTempPath(), "MagicalCryptoWallet-signatures-" + Guid.NewGuid());
		Directory.CreateDirectory(directory);
		try
		{
			var manifest = Path.Combine(directory, "SHA256SUMS.asc");
			var signature = Path.Combine(directory, "SHA256SUMS.magicalcryptowalletsig");
			await File.WriteAllTextAsync(manifest, "Synthetic manifest, no real wallet or release");
			using var key = new Key();
			var hash = new uint256(SHA256.HashData(await File.ReadAllBytesAsync(manifest)));
			await File.WriteAllTextAsync(signature, Convert.ToBase64String(key.Sign(hash).ToDER()));
			await ReleaseDownloader.VerifySha256SumsFileAsync(manifest, signature, CancellationToken.None, key.PubKey.ToHex());
			await Assert.ThrowsAsync<InvalidOperationException>(() => ReleaseDownloader.VerifySha256SumsFileAsync(manifest, signature, CancellationToken.None));
			await File.AppendAllTextAsync(manifest, "tampered");
			await Assert.ThrowsAsync<InvalidOperationException>(() => ReleaseDownloader.VerifySha256SumsFileAsync(manifest, signature, CancellationToken.None, key.PubKey.ToHex()));
		}
		finally { Directory.Delete(directory, recursive: true); }
	}
}
