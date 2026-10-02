// Development-only type shim for the unused ManagedApplicationHost.BindTermination
// overload. The test uses the actual Connect/Request/Dispose transport, and does
// not claim verification of the wallet's full termination service.
namespace MagicalCryptoWallet.Services.Terminate;
public sealed class TerminateService
{
    public void SignalForceTerminate() => throw new System.InvalidOperationException("Unused test-only termination type.");
}
