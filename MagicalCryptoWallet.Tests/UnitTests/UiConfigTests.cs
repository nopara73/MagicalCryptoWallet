using System.IO;
using System.Text.Json;
using System.Threading.Tasks;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Tests.Helpers;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class UiConfigTests
{
	[Fact]
	public async Task LoadingSettingsDoesNotScheduleUnrequestedWritesAsync()
	{
		string workDir = Common.GetWorkDir();
		Directory.CreateDirectory(workDir);
		string filePath = Path.Combine(workDir, $"{Guid.NewGuid():N}.json");
		new UiConfig(filePath).ToFile();
		var originalWrite = File.GetLastWriteTimeUtc(filePath);

		_ = UiConfig.LoadFile(filePath);
		await Task.Delay(1500);

		Assert.Equal(originalWrite, File.GetLastWriteTimeUtc(filePath));
	}

	[Fact]
	public async Task ConcurrentReadsAndSavesPreserveHiddenSettingsAsync()
	{
		string workDir = Common.GetWorkDir();
		Directory.CreateDirectory(workDir);
		string filePath = Path.Combine(workDir, $"{Guid.NewGuid():N}.json");
		var config = new UiConfig(filePath) { PrivacyMode = true };
		config.ToFile();

		await Task.WhenAll(
			Task.Run(() =>
			{
				for (int i = 0; i < 100; i++) { config.ToFile(); }
			}),
			Task.Run(() =>
			{
				for (int i = 0; i < 100; i++)
				{
					var loaded = UiConfig.LoadFile(filePath);
					Assert.True(loaded.PrivacyMode);
					loaded.ToFile();
				}
			}));
		Assert.True(UiConfig.LoadFile(filePath).PrivacyMode);
	}

	[Theory]
	[InlineData(false)]
	[InlineData(true)]
	public void LurkingWifeModePreservesExistingSavedSetting(bool enabled)
	{
		string workDir = Common.GetWorkDir();
		Directory.CreateDirectory(workDir);
		string filePath = Path.Combine(workDir, $"{Guid.NewGuid():N}.json");
		var config = new UiConfig(filePath) { PrivacyMode = enabled };
		config.ToFile();

		using (var persisted = JsonDocument.Parse(File.ReadAllText(filePath)))
		{
			// Keep the existing storage key so a name change never reveals a previously hidden wallet.
			Assert.Equal(enabled, persisted.RootElement.GetProperty("PrivacyMode").GetBoolean());
		}

		var reopened = UiConfig.LoadFile(filePath);
		Assert.Equal(enabled, reopened.PrivacyMode);
		reopened.PrivacyMode = !enabled;
		reopened.ToFile();
		Assert.Equal(!enabled, UiConfig.LoadFile(filePath).PrivacyMode);
	}

	[Theory]
	[InlineData(null)]
	[InlineData("{invalid json")]
	public void LoadFileCompletesDefaultFileWriteBeforeReturning(string? existingContent)
	{
		string workDir = Common.GetWorkDir();
		Directory.CreateDirectory(workDir);
		string filePath = Path.Combine(workDir, $"{Guid.NewGuid():N}.json");
		if (existingContent is not null)
		{
			File.WriteAllText(filePath, existingContent);
		}

		var config = UiConfig.LoadFile(filePath);

		using (var persisted = JsonDocument.Parse(File.ReadAllText(filePath)))
		{
			Assert.True(persisted.RootElement.GetProperty(nameof(UiConfig.Oobe)).GetBoolean());
			Assert.False(persisted.RootElement.GetProperty(nameof(UiConfig.RunOnSystemStartup)).GetBoolean());
		}

		// A subsequent save must not overlap the initial write or be overwritten by it.
		config.Oobe = false;
		config.ToFile();
		using var updated = JsonDocument.Parse(File.ReadAllText(filePath));
		Assert.False(updated.RootElement.GetProperty(nameof(UiConfig.Oobe)).GetBoolean());
	}
}
