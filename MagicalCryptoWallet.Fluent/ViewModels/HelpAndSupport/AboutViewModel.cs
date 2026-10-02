using System.Windows.Input;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Fluent.ViewModels.HelpAndSupport;

[NavigationMetaData(
	Title = "About Magical Crypto Wallet",
	Caption = "Display Magical Crypto Wallet's current info",
	IconName = "info_regular",
	Order = 0,
	Category = "Help & Support",
	Keywords = new[]
	{
		"About", "Software", "Version", "Source", "Code", "GitHub", "License", "Information", "Wallet"
	},
	NavBarPosition = NavBarPosition.None,
	NavigationTarget = NavigationTarget.DialogScreen)]
public partial class AboutViewModel : RoutableViewModel
{
	public AboutViewModel(UiContext uiContext, bool navigateBack = false) : base(uiContext)
	{
		EnableBack = navigateBack;

		SourceCode = new LinkViewModel(UiContext)
		{
			Link = SourceCodeLink,
			Description = "Source Code (GitHub)",
			IsClickable = true
		};

		License = new LinkViewModel(UiContext)
		{
			Link = LicenseLink,
			Description = "MIT License",
			IsClickable = true
		};

		ReleaseHighlightsDialogCommand = ReactiveCommand.CreateFromTask(async () => await Navigate().To().ReleaseHighlightsDialog().GetResultAsync());

		NextCommand = CancelCommand;

		SetupCancel(enableCancel: false, enableCancelOnEscape: true, enableCancelOnPressed: true);
	}

	public LinkViewModel SourceCode { get; }

	public LinkViewModel License { get; }

	public ICommand ReleaseHighlightsDialogCommand { get; }

	public Version ClientVersion => Constants.ClientVersion;

	public static string SourceCodeLink => "https://github.com/nopara73/MagicalCryptoWallet/";

	public static string LicenseLink => "https://github.com/nopara73/MagicalCryptoWallet/blob/master/LICENSE.md";
}
