using System.Runtime.InteropServices;
using MagicalCryptoWallet.Client;
using System.Threading.Tasks;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class StartupHelper
{
	public const string SilentArgument = StartupLaunch.SilentArgument;

	public static async Task ModifyStartupSettingAsync(bool runOnSystemStartup, StartupLaunch launch)
	{
		try
		{
			if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
			{
				WindowsStartupHelper.AddOrRemoveRegistryKey(runOnSystemStartup, launch: launch);
			}
			else if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
			{
				await LinuxStartupHelper.AddOrRemoveDesktopFileAsync(runOnSystemStartup, launch: launch).ConfigureAwait(false);
			}
			else if (RuntimeInformation.IsOSPlatform(OSPlatform.OSX))
			{
				await MacOsStartupHelper.AddOrRemoveStartupItemAsync(runOnSystemStartup, launch: launch).ConfigureAwait(false);
			}
		}
		catch (Exception ex)
		{
			// Suppress exception to avoid potential crashes.
			Logger.LogError($"{ex}");
		}
	}
}
