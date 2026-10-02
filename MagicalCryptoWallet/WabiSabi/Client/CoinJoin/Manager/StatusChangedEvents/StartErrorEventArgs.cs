namespace MagicalCryptoWallet.WabiSabi.Client.StatusChangedEvents;

public class StartErrorEventArgs : StatusChangedEventArgs
{
	public StartErrorEventArgs(CoinjoinError error)
	{
		Error = error;
	}

	public CoinjoinError Error { get; }
}
