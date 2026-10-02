using System.IO;
using MagicalCryptoWallet.Client;
using System.Threading.Tasks;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class LinuxStartupHelper
{
    public static async Task AddOrRemoveDesktopFileAsync(bool runOnSystemStartup, string? homeDirectory = null, StartupLaunch? launch = null)
	{
        string pathToDir = Path.Combine(homeDirectory ?? Environment.GetFolderPath(Environment.SpecialFolder.UserProfile), ".config", "autostart");
		string pathToDesktopFile = Path.Combine(pathToDir, Constants.ApplicationId + ".desktop");

		IoHelpers.EnsureContainingDirectoryExists(pathToDesktopFile);

		if (runOnSystemStartup)
		{
			launch ??= StartupLaunch.Default;
			string pathToExec = launch.Executable;

			string pathToExecWithArgs = launch.DesktopCommandLine;

			IoHelpers.EnsureFileExists(pathToExec);

			string fileContents = string.Join(
				"\n",
				"[Desktop Entry]",
				$"Name={Constants.AppName}",
				"Type=Application",
				$"Exec={pathToExecWithArgs}",
				"Hidden=false",
				"Terminal=false",
				"X-GNOME-Autostart-enabled=true");

			await File.WriteAllTextAsync(pathToDesktopFile, fileContents).ConfigureAwait(false);
		}
		else
		{
			File.Delete(pathToDesktopFile);
		}
	}
}
