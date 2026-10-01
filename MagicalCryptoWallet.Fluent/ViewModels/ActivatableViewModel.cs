using System.Reactive.Disposables;

namespace MagicalCryptoWallet.Fluent.ViewModels;

public class ActivatableViewModel(UiContext uiContext) : ViewModelBase(uiContext)
{
	protected virtual void OnActivated(CompositeDisposable disposables)
	{
	}

	public void Activate(CompositeDisposable disposables)
	{
		OnActivated(disposables);
	}
}
