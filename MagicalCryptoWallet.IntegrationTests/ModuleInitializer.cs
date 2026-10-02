using System.Runtime.CompilerServices;

namespace MagicalCryptoWallet.IntegrationTests;

public static class ModuleInitializer
{
	[ModuleInitializer]
	internal static void Initialize()
	{
		global::MagicalCryptoWallet.Tests.Infrastructure.McwManagedTestHost.Initialize();
		// Make sure that MagicalCryptoWallet.ModuleInitializer is initialized before running the tests.
		RuntimeHelpers.RunClassConstructor(typeof(MagicalCryptoWallet.ModuleInitializer).TypeHandle);
	}
}
