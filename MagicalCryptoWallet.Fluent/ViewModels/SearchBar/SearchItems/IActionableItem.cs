using System.Threading.Tasks;

namespace MagicalCryptoWallet.Fluent.ViewModels.SearchBar.SearchItems;

public interface IActionableItem : ISearchItem
{
	Func<Task> Activate { get; }
}
