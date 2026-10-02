using Microsoft.Win32;
using System.IO;
using System.Runtime.InteropServices;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class WindowsStartupHelper
{
	private const string KeyPath = "SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Run";

	public static void AddOrRemoveRegistryKey(bool runOnSystemStartup, string keyPath = KeyPath)
	{
		if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
		{
			throw new InvalidOperationException("Registry modification can only be done on Windows.");
		}

		string pathToExeFile = EnvironmentHelpers.GetExecutablePath();

		string pathToExecWithArgs = $"\"{pathToExeFile}\" {StartupHelper.SilentArgument}";

		if (!File.Exists(pathToExeFile))
		{
			throw new InvalidOperationException($"Path: {pathToExeFile} does not exist.");
		}

		using RegistryKey? key = runOnSystemStartup
			? Registry.CurrentUser.CreateSubKey(keyPath, writable: true)
			: Registry.CurrentUser.OpenSubKey(keyPath, writable: true);

		if (key is null)
		{
			if (runOnSystemStartup)
			{
				throw new InvalidOperationException("Registry operation failed.");
			}

			return;
		}

		var existingPath = key.GetValue(nameof(MagicalCryptoWallet));
		if (existingPath is null && runOnSystemStartup)
		{
			key.SetValue(nameof(MagicalCryptoWallet), pathToExecWithArgs);
		}
		else if (existingPath is not null && !runOnSystemStartup)
		{
			key.DeleteValue(nameof(MagicalCryptoWallet), false);
		}
	}
}
