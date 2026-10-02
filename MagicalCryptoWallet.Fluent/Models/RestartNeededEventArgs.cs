namespace MagicalCryptoWallet.Fluent.Models;

public class RestartNeededEventArgs : EventArgs
{
	public bool IsRestartNeeded { get; init; }
}
