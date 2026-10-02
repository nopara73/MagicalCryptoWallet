using System.IO;
using System.Runtime.CompilerServices;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Tests.Helpers;

namespace MagicalCryptoWallet.Tests;

public static class ModuleInitializer
{
	[ModuleInitializer]
	internal static void Initialize()
	{
		global::MagicalCryptoWallet.Tests.Infrastructure.McwManagedTestHost.Initialize();
		// Make sure that MagicalCryptoWallet.ModuleInitializer is initialized before running the tests.
		RuntimeHelpers.RunClassConstructor(typeof(MagicalCryptoWallet.ModuleInitializer).TypeHandle);

		Logger.Configure(Path.Combine(Common.DataDir, "Logs.txt"), LogLevel.Info, [LogMode.Debug, LogMode.File]);
	}
}
