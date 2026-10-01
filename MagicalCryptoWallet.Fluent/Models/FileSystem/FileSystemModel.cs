using System.Threading.Tasks;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Fluent.Models.FileSystem;

public class FileSystemModel
{
	public Task OpenFileInTextEditorAsync(string filePath)
	{
		return FileHelpers.OpenFileInTextEditorAsync(filePath);
	}

	public void OpenFolderInFileExplorer(string dirPath)
	{
		IoHelpers.OpenFolderInFileExplorer(dirPath);
	}
}
