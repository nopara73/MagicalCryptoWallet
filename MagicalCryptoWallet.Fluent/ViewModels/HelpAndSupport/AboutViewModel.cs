using System.Collections.Generic;
using System.Windows.Input;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Fluent.ViewModels.HelpAndSupport;

[NavigationMetaData(
	Title = "About Magical Crypto Wallet",
	Caption = "Display Magical Crypto Wallet's current info",
	IconName = "info_regular",
	Order = 4,
	Category = "Help & Support",
	Keywords = new[]
	{
			"About", "Software", "Version", "Source", "Code", "Github", "Website", "Coordinator", "Status", "Stats", "Tor", "Onion",
			"User", "Support", "Bug", "Report", "FAQ", "Questions,", "Docs", "Documentation", "License", "Advanced", "Information",
			"Hardware", "Wallet"
	},
	NavBarPosition = NavBarPosition.None,
	NavigationTarget = NavigationTarget.DialogScreen)]
public partial class AboutViewModel : RoutableViewModel
{
	public AboutViewModel(UiContext uiContext, bool navigateBack = false) : base(uiContext)
	{
		EnableBack = navigateBack;

		Links = new List<ViewModelBase>()
			{
				new LinkViewModel(UiContext)
				{
					Link = DocsLink,
					Description = "Documentation",
					IsClickable = true
				},
				new SeparatorViewModel(UiContext),
				new LinkViewModel(UiContext)
				{
					Link = SourceCodeLink,
					Description = "Source Code (GitHub)",
					IsClickable = true
				},
				new SeparatorViewModel(UiContext),
				new LinkViewModel(UiContext)
				{
					Link = ClearnetLink,
					Description = "Website (Clearnet)",
					IsClickable = true
				},
				new SeparatorViewModel(UiContext),
				new LinkViewModel(UiContext)
				{
					Link = UserSupportLink,
					Description = "User Support",
					IsClickable = true
				},
				new SeparatorViewModel(UiContext),
				new LinkViewModel(UiContext)
				{
					Link = BugReportLink,
					Description = "Bug Report",
					IsClickable = true
				},
				new SeparatorViewModel(UiContext),
				new LinkViewModel(UiContext)
				{
					Link = FAQLink,
					Description = "FAQ",
					IsClickable = true
				},
			};

		License = new LinkViewModel(UiContext)
		{
			Link = LicenseLink,
			Description = "MIT License",
			IsClickable = true
		};

		OpenBrowserCommand = ReactiveCommand.CreateFromTask<string>(x => UiContext.OpenBrowserAsync(x));

		ReleaseHighlightsDialogCommand = ReactiveCommand.CreateFromTask(async () => await Navigate().To().ReleaseHighlightsDialog().GetResultAsync());

		CopyLinkCommand = ReactiveCommand.CreateFromTask<string>(async (link) => await UiContext.Clipboard.SetTextAsync(link));

		NextCommand = CancelCommand;

		SetupCancel(enableCancel: false, enableCancelOnEscape: true, enableCancelOnPressed: true);
	}

	public List<ViewModelBase> Links { get; }

	public LinkViewModel License { get; }

	public ICommand ReleaseHighlightsDialogCommand { get; }

	public ICommand OpenBrowserCommand { get; }

	public ICommand CopyLinkCommand { get; }

	public Version ClientVersion => Constants.ClientVersion;

	public static string ClearnetLink => "https://github.com/nopara73/MagicalCryptoWallet/releases";


	public static string SourceCodeLink => "https://github.com/nopara73/MagicalCryptoWallet/";

	public static string UserSupportLink => "https://github.com/nopara73/MagicalCryptoWallet/issues";

	public static string BugReportLink => "https://github.com/nopara73/MagicalCryptoWallet/issues/new?template=bug-report.md";

	public static string FAQLink => "https://github.com/nopara73/MagicalCryptoWallet/blob/master/MagicalCryptoWallet.Documentation/README.md#coordinators";

	public static string DocsLink => "https://github.com/nopara73/MagicalCryptoWallet/blob/master/MagicalCryptoWallet.Documentation/README.md";

	public static string LicenseLink => "https://github.com/nopara73/MagicalCryptoWallet/blob/master/LICENSE.md";
}
