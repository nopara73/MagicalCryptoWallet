using System.Windows.Input;

namespace MagicalCryptoWallet.Fluent.ViewModels.HelpAndSupport;

[NavigationMetaData(
	Title = "User Guide/Docs",
	Caption = "Open Magical Crypto Wallet's documentation website",
	Order = 2,
	Category = "Help & Support",
	Keywords = new[]
	{
		"User", "Support", "Website", "Docs", "Documentation", "Guide"
	},
	IconName = "book_question_mark_regular")]
public partial class DocsLinkViewModel : TriggerCommandViewModel
{
	public DocsLinkViewModel(UiContext uiContext) : base(uiContext)
	{
		TargetCommand = ReactiveCommand.CreateFromTask(async () => await UiContext.OpenBrowserAsync(AboutViewModel.DocsLink));
	}

	public override ICommand TargetCommand { get; }
}
