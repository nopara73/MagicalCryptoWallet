using System.Reactive.Disposables;
using Avalonia;
using Avalonia.Xaml.Interactions.Custom;
using MagicalCryptoWallet.Fluent.Helpers;

namespace MagicalCryptoWallet.Fluent.Behaviors;

public class RegisterNotificationHostBehavior : AttachedToVisualTreeBehavior<Visual>
{
	protected override IDisposable OnAttachedToVisualTreeOverride()
	{
		if (AssociatedObject is null)
		{
			return Disposable.Empty;
		}

		NotificationHelpers.SetNotificationManager(AssociatedObject);

		return Disposable.Empty;
	}
}
