namespace MagicalCryptoWallet.Fluent.Models;

public enum HealthMonitorState
{
	Loading,
	Ready,
	UpdateAvailable,
	BitcoinRpcIssueDetected,
	BitcoinRpcSynchronizing,
}
