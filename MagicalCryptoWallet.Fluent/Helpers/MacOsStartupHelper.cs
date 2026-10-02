using System.IO;
using System.Threading.Tasks;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class MacOsStartupHelper
{
	private static readonly string PlistContent =
		$"""
		<?xml version="1.0" encoding="UTF-8"?>
		<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
		<plist version="1.0">
		<dict>
		    <key>Label</key>
		    <string>{Constants.ApplicationId}.startup</string>
			<key>ProgramArguments</key>
			<array>
				<string>{System.Security.SecurityElement.Escape(EnvironmentHelpers.GetExecutablePath())}</string>
				<string>{StartupHelper.SilentArgument}</string>
			</array>
			<key>RunAtLoad</key>
			<true/>
		</dict>
		</plist>
		""";

    public static async Task AddOrRemoveStartupItemAsync(bool runOnSystemStartup, string? homeDirectory = null)
	{

        string homeDir = homeDirectory ?? Environment.GetFolderPath(Environment.SpecialFolder.UserProfile);
		var libraryDir = Path.Combine(homeDir, "Library");
		var launchAgentsDir = Path.Combine(libraryDir, "LaunchAgents");
		var plistPath = Path.Combine(launchAgentsDir, Constants.SilentPlistName);

		if (runOnSystemStartup)
		{
			if (!Directory.Exists(libraryDir))
			{
				Logger.LogInfo("Creating Library directory because it doesn't exist.");
				Directory.CreateDirectory(libraryDir);
			}

			if (!Directory.Exists(launchAgentsDir))
			{
				Logger.LogInfo("Creating LaunchAgents directory because it doesn't exist.");
				Directory.CreateDirectory(launchAgentsDir);
			}

			await File.WriteAllTextAsync(plistPath, PlistContent).ConfigureAwait(false);
		}
		else if (File.Exists(plistPath))
		{
			File.Delete(plistPath);
		}
	}

}
