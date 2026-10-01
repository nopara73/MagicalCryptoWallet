using System.Runtime.InteropServices;

namespace MagicalCryptoWallet.Helpers;

public static partial class WindowsAppIdentity
{
	public static void Apply()
	{
		if (OperatingSystem.IsWindows())
		{
			Marshal.ThrowExceptionForHR(SetCurrentProcessExplicitAppUserModelID(Constants.ApplicationId));
		}
	}

	[LibraryImport("shell32.dll", StringMarshalling = StringMarshalling.Utf16)]
	private static partial int SetCurrentProcessExplicitAppUserModelID(string appId);
}
