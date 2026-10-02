using System.Windows.Input;
using MagicalCryptoWallet.Fluent.Extensions;

namespace MagicalCryptoWallet.Fluent.ViewModels.OpenDirectory;

public abstract class OpenFileViewModel(UiContext uiContext) : TriggerCommandViewModel(uiContext)
{
	public abstract string FilePath { get; }

	public override ICommand TargetCommand =>
		ReactiveCommand.CreateFromTask(async () =>
		{
			try
			{
				await UiContext.FileSystem.OpenFileInTextEditorAsync(FilePath);
			}
			catch (Exception ex)
			{
				await ShowErrorAsync("Open", ex.ToUserFriendlyString(), "Magical Crypto Wallet was unable to open the file");
			}
		});
}
