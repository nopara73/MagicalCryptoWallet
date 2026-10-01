using MagicalCryptoWallet.Fluent.Models.UI;

namespace MagicalCryptoWallet.Fluent.ViewModels.OpenDirectory;

[NavigationMetaData(
	Title = "Config File",
	Caption = "",
	Order = 4,
	Category = "Open",
	Keywords = new[]
	{
			"Browse", "Open", "Config", "File"
	},
	IconName = "document_regular")]
public partial class OpenConfigFileViewModel : OpenFileViewModel
{
	public OpenConfigFileViewModel(UiContext uiContext) : base(uiContext)
	{
	}

	public override string FilePath => UiContext.Config.ConfigFilePath;
}
