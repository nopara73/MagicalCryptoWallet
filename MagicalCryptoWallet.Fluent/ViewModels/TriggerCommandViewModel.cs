using System.Windows.Input;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;

namespace MagicalCryptoWallet.Fluent.ViewModels;

public abstract class TriggerCommandViewModel(UiContext uiContext) : RoutableViewModel(uiContext)
{
	public abstract ICommand TargetCommand { get; }
}
