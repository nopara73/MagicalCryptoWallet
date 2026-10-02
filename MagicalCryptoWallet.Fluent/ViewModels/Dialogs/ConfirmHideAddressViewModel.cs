using ReactiveUI;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.Base;

namespace MagicalCryptoWallet.Fluent.ViewModels.Dialogs;

[NavigationMetaData(Title = "Hide Address", NavigationTarget = NavigationTarget.CompactDialogScreen)]
public partial class ConfirmHideAddressViewModel : DialogViewModelBase<bool>
{
	public ConfirmHideAddressViewModel(UiContext uiContext, LabelsArray labels) : base(uiContext)
	{
		Labels = labels;

		NextCommand = ReactiveCommand.Create(() => Close(result: true));
		CancelCommand = ReactiveCommand.Create(() => Close(DialogResultKind.Cancel));

		SetupCancel(enableCancel: false, enableCancelOnEscape: true, enableCancelOnPressed: true);
	}

	public LabelsArray Labels { get; }
}
