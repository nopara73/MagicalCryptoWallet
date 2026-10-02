using DynamicData;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar.Patterns;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar.SearchItems;

namespace MagicalCryptoWallet.Fluent.ViewModels.SearchBar.Sources;

public interface ISearchSource
{
	IObservable<IChangeSet<ISearchItem, ComposedKey>> Changes { get; }
}
