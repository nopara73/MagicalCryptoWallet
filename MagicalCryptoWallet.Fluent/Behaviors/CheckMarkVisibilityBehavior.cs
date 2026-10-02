using Avalonia;
using Avalonia.Controls;
using System.Reactive.Disposables;
using System.Reactive.Linq;
using MagicalCryptoWallet.Fluent.Extensions;
using Avalonia.VisualTree;
using Avalonia.Xaml.Interactions.Custom;

namespace MagicalCryptoWallet.Fluent.Behaviors;

public class CheckMarkVisibilityBehavior : AttachedToVisualTreeBehavior<PathIcon>
{
	protected override IDisposable OnAttachedToVisualTreeOverride()
	{
		if (AssociatedObject is null)
		{
			return Disposable.Empty;
		}

		var ownerTextBox =
			AssociatedObject.FindAncestorOfType<TextBox>();

		if (ownerTextBox is null)
		{
			return Disposable.Empty;
		}

		var hasErrors = ownerTextBox.GetObservable(DataValidationErrors.HasErrorsProperty);
		var text = ownerTextBox.GetObservable(TextBox.TextProperty);

		return hasErrors.ToSignal()
			.Merge(text.ToSignal())
			.Subscribe(_ =>
			{
				if (AssociatedObject is { })
				{
					AssociatedObject.IsVisible =
						!DataValidationErrors.GetHasErrors(ownerTextBox) &&
						!string.IsNullOrEmpty(ownerTextBox.Text);
				}
			});
	}
}
