using System.Windows.Input;
using ReactiveUI;
using MagicalCryptoWallet.Fluent.Helpers;

namespace MagicalCryptoWallet.Fluent.ViewModels.SearchBar.Settings;

public class RestartViewModel : ViewModelBase
{
	public RestartViewModel(UiContext uiContext, string message) : base(uiContext)
	{
		Message = message;
		RestartCommand = ReactiveCommand.Create(() => AppLifetimeHelper.Shutdown(withShutdownPrevention: true, restart: true));
	}

	public string Message { get; }

	public ICommand RestartCommand { get; }
}
