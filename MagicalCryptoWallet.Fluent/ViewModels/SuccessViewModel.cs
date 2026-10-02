using System.Reactive.Disposables;
using System.Threading.Tasks;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;

namespace MagicalCryptoWallet.Fluent.ViewModels;

[NavigationMetaData(Title = "Success")]
public partial class SuccessViewModel : RoutableViewModel
{
	public SuccessViewModel(UiContext uiContext) : base(uiContext)
	{
		NextCommand = ReactiveCommand.CreateFromTask(OnNextAsync);

		SetupCancel(enableCancel: false, enableCancelOnEscape: true, enableCancelOnPressed: true);
	}

	private async Task OnNextAsync()
	{
		await Task.Delay(UiConstants.CloseSuccessDialogMillisecondsDelay);

		Navigate().Clear();
	}

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		base.OnNavigatedTo(isInHistory, disposables);

		if (NextCommand is not null && NextCommand.CanExecute(default))
		{
			NextCommand.Execute(default);
		}
	}
}
