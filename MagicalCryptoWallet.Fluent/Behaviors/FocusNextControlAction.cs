using Avalonia;
using Avalonia.Input;
using Avalonia.Xaml.Interactivity;
using MagicalCryptoWallet.Fluent.Helpers;

namespace MagicalCryptoWallet.Fluent.Behaviors;

public class FocusNextControlAction : AvaloniaObject, IAction
{
	public object Execute(object? sender, object? parameter)
	{
		if (ApplicationHelper.FocusManager is { } focusManager)
		{
			var current = focusManager.GetFocusedElement();
			if (current != null)
			{
				var next = KeyboardNavigationHandler.GetNext(current, NavigationDirection.Next);
				return next?.Focus() ?? false;
			}
		}

		return false;
	}
}
