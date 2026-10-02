using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.Extensions;

public static class EventHandlerExtensions
{
	public static void SafeInvoke<T>(this EventHandler<T>? handler, object sender, T args) where T : class
	{
		if (handler is null) { return; }
		foreach (EventHandler<T> observer in handler.GetInvocationList())
		{
			try { observer(sender, args); }
			catch (Exception e) { Logger.LogWarning(e); }
		}
	}
}
