using System.Diagnostics;
using System.IO;
using System.Net.Http;
using System.Runtime.InteropServices;
using System.Security.Cryptography;
using NBitcoin.Crypto;
using NNostr.Client;
using MagicalCryptoWallet.Mcw.Network;
using MagicalCryptoWallet.BundledApps;
using MagicalCryptoWallet.WebClients;
using static MagicalCryptoWallet.Services.UpdateManager;

namespace MagicalCryptoWallet.Services;

// The Downloader
public delegate Task AsyncReleaseDownloader(ReleaseInfo releaseInfo, CancellationToken cancellationToken);

/// <summary>
/// Manages software updates by periodically checking for new releases via Nostr
/// </summary>
public static class UpdateManager
{
	public record UpdateMessage;

	public static MessageHandler<UpdateMessage, Unit> CreateUpdater(Func<INostrClient> nostrClientFactory,
		AsyncReleaseDownloader releaseDownloader, EventBus eventBus, Version? currentVersion = null,
		string announcementNpub = Constants.ReleaseAnnouncementNpub) =>
		(_, _, cancellationToken) => UpdateAsync(nostrClientFactory, releaseDownloader, eventBus, currentVersion ?? Constants.ClientVersion, announcementNpub, cancellationToken);

	private static async Task<Unit> UpdateAsync(Func<INostrClient> nostrClientFactory, AsyncReleaseDownloader releaseDownloader, EventBus eventBus, Version currentVersion, string announcementNpub, CancellationToken cancellationToken)
	{
		using var nostrClient = nostrClientFactory();
		using var magicalcryptowalletNostrClient = new MagicalCryptoWalletNostrClient(nostrClient, announcementNpub);
		try
		{
			// Connect to Nostr relays and check for release version updates
			await magicalcryptowalletNostrClient.ConnectAndSubscribeAsync(cancellationToken).ConfigureAwait(false);
			await ProcessReleaseEventsAsync(magicalcryptowalletNostrClient, releaseDownloader, eventBus, currentVersion, cancellationToken)
				.ConfigureAwait(false);
		}
		catch (AggregateException e)
		{
			Logger.LogWarning($"It was not possible to check for updates. {e.Message}");
		}
		finally
		{
			// Ensure we disconnect regardless of the outcome
			await magicalcryptowalletNostrClient.DisconnectAsync(cancellationToken).ConfigureAwait(false);
		}

		return Unit.Instance;
	}

	private static async Task ProcessReleaseEventsAsync(MagicalCryptoWalletNostrClient magicalcryptowalletNostrClient, AsyncReleaseDownloader releaseDownloader, EventBus eventBus, Version currentVersion, CancellationToken cancellationToken)
	{
		using var sixtySeconds = new CancellationTokenSource(TimeSpan.FromSeconds(60));
		using var linkedCancellationTokenSource = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, sixtySeconds.Token);

		try
		{
			// Read all the events as an array
			var releases = await magicalcryptowalletNostrClient.EventsReader
				.ReadAllAsync(linkedCancellationTokenSource.Token)
				.ToArrayAsync(linkedCancellationTokenSource.Token)
				.ConfigureAwait(false);

			// Find release with version greater than current version
			var latestRelease = releases
				.Where(x => x.Version > currentVersion)
				.MaxBy(x => x.Version);

			if(latestRelease is not null)
			{
				Logger.LogInfo($"New version found: {latestRelease.Version}");

				// Notify about new version via event bus
				var updateStatus = new UpdateStatus(ClientVersion: latestRelease.Version, ClientUpToDate: false, IsReadyToInstall: false);
				eventBus.Publish(new NewSoftwareVersionAvailable(updateStatus));

				// Download the new version
				await releaseDownloader(latestRelease, cancellationToken).ConfigureAwait(false);
			}
		}
		catch (OperationCanceledException) // Cancelled task is something we expect
		{
			Logger.LogInfo("No new Magical Crypto Wallet release was found.");
		}
	}

	public record UpdateStatus(bool ClientUpToDate, bool IsReadyToInstall, Version ClientVersion);
}



// Downloads and verifies new software releases
public static class ReleaseDownloader
{
	private static readonly UserAgentPicker UserAgentGetter = UserAgent.GenerateUserAgentPicker();

	public static AsyncReleaseDownloader ForOfficiallySupportedOSes(IMcwHttpClientFactory httpClientFactory, EventBus eventBus) =>
		ForOfficiallySupportedOSes(httpClientFactory, eventBus, GetInstallerName);

	internal static AsyncReleaseDownloader ForOfficiallySupportedOSes(
		IMcwHttpClientFactory httpClientFactory,
		EventBus eventBus,
		Func<Version, string> getInstallerName,
		string publicKey = Constants.UpdateSignaturePublicKey) =>
		(releaseInfo, cancellationToken) => DownloadNewMagicalCryptoWalletReleaseVersionAsync(
			httpClientFactory,
			eventBus,
			releaseInfo,
			getInstallerName(releaseInfo.Version),
			publicKey,
			cancellationToken);

	public static AsyncReleaseDownloader ForUnsupportedLinuxDistributions() =>
		(_, _) =>
		{
			Logger.LogInfo("For Linux, get the correct update manually.");
			return Task.CompletedTask;
		};

	public static AsyncReleaseDownloader AutoDownloadOff() =>
		(_, _) =>
		{
			Logger.LogInfo("Auto Download is turned off. Get the correct update manually.");
			return Task.CompletedTask;
		};

	// Downloads and verifies a new MagicalCryptoWallet release version
	private static async Task DownloadNewMagicalCryptoWalletReleaseVersionAsync(
		IMcwHttpClientFactory httpClientFactory,
		EventBus eventBus,
		ReleaseInfo releaseInfo,
		string installerFileName,
		string publicKey,
		CancellationToken cancellationToken)
	{
		var installDirectory = GetInstallDirectory(releaseInfo);

		// Download signature files in parallel
		var sha256SumsTask    = DownloadFileAsync(releaseInfo.Assets["SHA256SUMS"]);
		var sha256SumsAscTask = DownloadFileAsync(releaseInfo.Assets["SHA256SUMS.asc"]);
		var signatureTask = DownloadFileAsync(releaseInfo.Assets["SHA256SUMS.magicalcryptowalletsig"]);
		await Task.WhenAll(sha256SumsTask, sha256SumsAscTask, signatureTask).ConfigureAwait(false);

		// Verify signatures
		await VerifySha256SumsFileAsync(sha256SumsAscTask.Result, signatureTask.Result, cancellationToken, publicKey).ConfigureAwait(false);
		await VerifyManifestContentAsync(sha256SumsTask.Result, sha256SumsAscTask.Result, cancellationToken).ConfigureAwait(false);

		Logger.LogInfo("Trying to download new version.");

		// Find appropriate installer for current platform
		var installerUriResult = GetInstallerUri(installerFileName);
		if (!installerUriResult.IsOk)
		{
			Logger.LogError(installerUriResult.Error);
			installDirectory.Delete(true);
			return;
		}

		var installerUri = installerUriResult.Value;

		if (!installerUri.Scheme.StartsWith("http") || !installerUri.IsAbsoluteUri)
		{
			Logger.LogError($"Can't download installer file '{installerFileName}' from '{installerUri}'. Only absolute http url are supported.");
			installDirectory.Delete(true);
			return;
		}

		var installerFilePath = await DownloadFileAsync(installerUri).ConfigureAwait(false);

		Logger.LogInfo($"Installer downloaded to: {installerFilePath}");

		// Verify installer hash match the expected one
		var installerHash = await GetExpectedInstallerHashAsync().ConfigureAwait(false);

		await VerifyInstallerHashAsync(installerFilePath, installerHash, cancellationToken).ConfigureAwait(false);
		Logger.LogInfo("Installer verified successfully");

		// Notify UI that there is an installer ready.
		var updateStatus = new UpdateStatus(ClientVersion: releaseInfo.Version, ClientUpToDate: false, IsReadyToInstall: true);
		eventBus.Publish(new NewSoftwareVersionAvailable(updateStatus));

		// Set installer file path, so on exit we can launch the installer.
		eventBus.Publish(new NewSoftwareVersionInstallerAvailable(installerFilePath));
		return;

		Task<string> DownloadFileAsync(Uri uri)
		{
			var filePath = Path.Combine(installDirectory.FullName, uri.Segments[^1]);
			return File.Exists(filePath)
				? Task.FromResult(filePath)
				: DownloadAsync(httpClientFactory, uri, filePath, cancellationToken);
		}

		Result<Uri, string> GetInstallerUri(string filename) =>
			releaseInfo.Assets.TryGetValue(filename, out var uri)
			? uri
			: Result<Uri, string>.Fail($"There is no file '{filename}'.");

		async Task<string> GetExpectedInstallerHashAsync()
		{
			var lines = await File.ReadAllLinesAsync(sha256SumsAscTask.Result, cancellationToken).ConfigureAwait(false);
			var s = lines.Select(l => l.Split("  ./", StringSplitOptions.TrimEntries | StringSplitOptions.RemoveEmptyEntries))
				.Where(a => a.Length == 2)
				.Select(a => (Hash: a[0], FileName: a[1]))
				.FirstOrDefault(a => a.FileName == installerFileName)
				.Hash ?? throw new InvalidOperationException($"{installerFileName} was not found.");
			return s;
		}
	}

	private static DirectoryInfo GetInstallDirectory(ReleaseInfo releaseInfo)
	{
		var installDirectoryPath = Path.Combine(Path.GetTempPath(), $"magicalcryptowallet-installer-{releaseInfo.Version}");
		var installDirectory = Directory.CreateDirectory(installDirectoryPath);
		return installDirectory;
	}

	private static async Task<string> DownloadAsync(IMcwHttpClientFactory httpClientFactory, Uri uri, string filePath, CancellationToken cancellationToken)
	{
		File.Delete(filePath);
		var httpClient = httpClientFactory.CreateClient($"{uri.Host}-installers");
		httpClient.DefaultRequestHeaders.TryAddWithoutValidation("User-Agent", UserAgentGetter());
		using var request = new HttpRequestMessage(HttpMethod.Get, uri);
		var response = await httpClient.SendAsync(request, cancellationToken).ConfigureAwait(false);
		response.EnsureSuccessStatusCode();
		var contentStream = await response.Content.ReadAsStreamAsync(cancellationToken).ConfigureAwait(false);
		using var fileStream = new FileStream(filePath, FileMode.Create);
		await contentStream.CopyToAsync(fileStream, cancellationToken).ConfigureAwait(false);
		return filePath;
	}

	internal static async Task VerifySha256SumsFileAsync(string sha256SumsAscFilePath, string signatureFilePath,
		CancellationToken cancellationToken, string publicKey = Constants.UpdateSignaturePublicKey)
	{
		// Read the content file
		byte[] bytes = await File.ReadAllBytesAsync(sha256SumsAscFilePath, cancellationToken).ConfigureAwait(false);
		var computedHash = new uint256(SHA256.HashData(bytes));

		// Read the signature file
		var signatureText = await File.ReadAllTextAsync(signatureFilePath, cancellationToken).ConfigureAwait(false);
		var signatureBytes = Convert.FromBase64String(signatureText);

		var signature = ECDSASignature.FromDER(signatureBytes);

		var pubKey = new PubKey(publicKey);

		if (!pubKey.Verify(computedHash, signature))
		{
			throw new InvalidOperationException("Invalid Magical Crypto Wallet update signature.");
		}
	}

	internal static async Task VerifyManifestContentAsync(string plainFile, string signedFile, CancellationToken cancellationToken)
	{
		var plain = (await File.ReadAllTextAsync(plainFile, cancellationToken).ConfigureAwait(false)).Replace("\r\n", "\n").TrimEnd();
		var armored = (await File.ReadAllTextAsync(signedFile, cancellationToken).ConfigureAwait(false)).Replace("\r\n", "\n");
		var start = armored.IndexOf("\n\n", StringComparison.Ordinal);
		var end = armored.IndexOf("-----BEGIN PGP SIGNATURE-----", StringComparison.Ordinal);
		if (!armored.StartsWith("-----BEGIN PGP SIGNED MESSAGE-----", StringComparison.Ordinal) || start < 0 || end <= start)
		{
			throw new InvalidOperationException("Invalid signed checksum manifest.");
		}
		var content = string.Join("\n", armored[(start + 2)..end].Split('\n').Select(line => line.StartsWith("- ", StringComparison.Ordinal) ? line[2..] : line)).TrimEnd();
		if (content != plain)
		{
			throw new InvalidOperationException("The checksum manifest differs from its signed contents.");
		}
	}

	private static async Task VerifyInstallerHashAsync(string installerFilePath, string expectedHash, CancellationToken cancellationToken)
	{
		var bytes1 = await File.ReadAllBytesAsync(installerFilePath, cancellationToken).ConfigureAwait(false);
		var computedHash = SHA256.HashData(bytes1);
		var downloadedHash = Convert.ToHexString(computedHash).ToLower();

		if (expectedHash != downloadedHash)
		{
			throw new InvalidOperationException("Downloaded file hash doesn't match expected hash.");
		}
	}

	private static string GetInstallerName(Version version) =>
		GetInstallerName(
			version,
			PlatformInformation.GetOsPlatform(),
			RuntimeInformation.ProcessArchitecture,
			PlatformInformation.IsDebianBasedOS());

	internal static string GetInstallerName(Version version, OS platform, Architecture architecture, bool isDebianBased) =>
		(platform, architecture, isDebianBased) switch
		{
			(OS.Windows, _, _) => $"MagicalCryptoWallet-{version}.msi",
			(OS.OSX, Architecture.Arm64, _) => $"MagicalCryptoWallet-{version}-arm64.dmg",
			(OS.OSX, _, _) => $"MagicalCryptoWallet-{version}.dmg",
			(OS.Linux, Architecture.Arm64, true) => $"MagicalCryptoWallet-{version}-arm64.deb",
			(OS.Linux, Architecture.X64, true) => $"MagicalCryptoWallet-{version}.deb",
			(OS.Linux, Architecture.X64, false) => $"MagicalCryptoWallet-{version}-linux-x64.tar.gz",
			(OS.Linux, Architecture.Arm64, false) => $"MagicalCryptoWallet-{version}-linux-arm64.tar.gz",
			_ => throw new NotSupportedException($"Unsupported platform: '{RuntimeInformation.OSDescription}'.")
		};

}

public static class Installer
{
	public static void StartInstallingNewVersion(string installerPath)
	{
		try
		{
			ProcessStartInfo startInfo;
			if (!File.Exists(installerPath))
			{
				throw new FileNotFoundException(installerPath);
			}
			if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
			{
				startInfo = ProcessStartInfoFactory.Make(installerPath, [], true);
			}
			else
			{
				startInfo = new()
				{
					FileName = installerPath,
					UseShellExecute = true,
					WindowStyle = ProcessWindowStyle.Normal
				};
			}

			using var p = Process.Start(startInfo);

			if (p is null)
			{
				throw new InvalidOperationException($"Can't start {nameof(p)} {startInfo.FileName}.");
			}
			if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
			{
				// For MacOS, you need to start the process twice, first start => permission denied
				// TODO: find out why and fix.

				p!.WaitForExit(5000);
				p.Start();
			}
		}
		catch (Exception ex)
		{
			Logger.LogError("Failed to install latest release. File might be corrupted.", ex);
		}
	}
}
