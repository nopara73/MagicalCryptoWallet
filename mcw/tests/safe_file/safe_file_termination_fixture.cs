// Only the actual host adapter's unused termination hook is stubbed in this
// development harness. File I/O, framing and registration use real source.
namespace MagicalCryptoWallet.Services.Terminate;
public sealed class TerminateService
{
	public void SignalForceTerminate() => throw new System.InvalidOperationException("Unexpected synthetic host termination.");
}
