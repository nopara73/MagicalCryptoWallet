using System.Linq;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar.SearchItems;

namespace MagicalCryptoWallet.Fluent.ViewModels.SearchBar.Sources;

public static class SearchSource
{
	public static Func<ISearchItem, bool> DefaultFilter(string query)
	{
		return item =>
		{
			if (string.IsNullOrWhiteSpace(query))
			{
				return item.IsDefault;
			}

			return new[] { item.Name, item.Description, }.Concat(item.Keywords)
				.Any(s => s.Contains(query, StringComparison.InvariantCultureIgnoreCase));
		};
	}
}
