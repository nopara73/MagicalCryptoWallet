using System.Windows.Input;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.HelpAndSupport;

[NavigationMetaData(
	Title = "Find a Coordinator",
	Caption = "Open MagicalCryptoWallet's documentation website",
	Order = 3,
	Category = "Help & Support",
	Keywords =
	[
		"Find", "Coordinator", "Coinjoin", "Docs", "Documentation", "Guide"
	],
	IconName = "book_question_mark_regular")]
public partial class FindCoordinatorLinkViewModel : TriggerCommandViewModel
{
	public FindCoordinatorLinkViewModel(UiContext uiContext) : base(uiContext)
	{
		TargetCommand = ReactiveCommand.CreateFromTask(async () => await UiContext.OpenBrowserAsync(WalletViewModel.FindCoordinatorLink));
	}

	public override ICommand TargetCommand { get; }
}
