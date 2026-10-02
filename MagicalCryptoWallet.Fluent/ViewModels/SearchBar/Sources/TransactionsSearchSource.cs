using System.Collections.Generic;
using System.Diagnostics.CodeAnalysis;
using System.Linq;
using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.ViewModels.NavBar;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar.Patterns;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar.SearchItems;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.History.HistoryItems;

namespace MagicalCryptoWallet.Fluent.ViewModels.SearchBar.Sources;

public class TransactionsSearchSource : ReactiveObject, ISearchSource, IDisposable
{
	private const int MaxResultCount = 5;
	private const int MinQueryLength = 3;

	private readonly CompositeDisposable _disposables = new();
	private readonly NavBarViewModel _navBarViewModel;

	public TransactionsSearchSource(NavBarViewModel navBarViewModel, IObservable<string> queries)
	{
		_navBarViewModel = navBarViewModel;

#pragma warning disable CA2000 // Dispose objects before losing scope - disposed with the disposables
		var sourceCache = new SourceCache<ISearchItem, ComposedKey>(x => x.Key)
			.DisposeWith(_disposables);
#pragma warning restore CA2000 // Dispose objects before losing scope

		var results = queries.CombineLatest(_navBarViewModel.UiContext.ApplicationSettings.WhenAnyValue(x => x.PrivacyMode), (query, _) => query)
			.Select(query => query.Length >= MinQueryLength ? Search(query) : [])
			.ObserveOn(RxApp.MainThreadScheduler);

		sourceCache
			.RefillFrom(results)
			.DisposeWith(_disposables);

		Changes = sourceCache.Connect();
	}

	public void Dispose()
	{
		_disposables.Dispose();
	}

	public IObservable<IChangeSet<ISearchItem, ComposedKey>> Changes { get; }

	private static bool ContainsId(HistoryItemViewModelBase historyItemViewModelBase, string queryStr)
	{
		return historyItemViewModelBase.Transaction.Id.ToString().Contains(queryStr, StringComparison.CurrentCultureIgnoreCase);
	}

	private Task NavigateTo(WalletViewModel wallet, HistoryItemViewModelBase item)
	{
		wallet.NavigateAndHighlight(item.Transaction.Id);

		return Task.CompletedTask;
	}

	private static string GetIcon(HistoryItemViewModelBase historyItemViewModelBase)
	{
		return historyItemViewModelBase switch
		{
			CoinJoinHistoryItemViewModel => "shield_regular",
			CoinJoinsHistoryItemViewModel => "shield_regular",
			TransactionHistoryItemViewModel => "normal_transaction",
			_ => ""
		};
	}

	private ISearchItem ToSearchItem(WalletViewModel wallet, HistoryItemViewModelBase item)
	{
		return new ActionableItem(
			item.Transaction.Id.ToString(),
			"Wallet transaction",
			() => NavigateTo(wallet, item),
			"Transactions",
			new List<string>())
		{
			Icon = GetIcon(item)
		};
	}

	private IEnumerable<(WalletViewModel Wallet, HistoryItemViewModelBase Transaction)> GetTransactions()
	{
		if (_navBarViewModel.Home is { WalletModel.SessionStatus.HasCachedData: true } wallet && !wallet.UiContext.ApplicationSettings.PrivacyMode)
		{
			foreach (var transaction in wallet.History.Transactions.Concat(wallet.History.Transactions.OfType<CoinJoinsHistoryItemViewModel>().SelectMany(x => x.Children)))
			{
				yield return (wallet, transaction);
			}
		}
	}

	private IEnumerable<ISearchItem> Search(string query)
	{
		return Filter(query)
			.Take(MaxResultCount)
			.Select(tuple => ToSearchItem(tuple.Item1, tuple.Item2));
	}

	private IEnumerable<(WalletViewModel, HistoryItemViewModelBase)> Filter(string queryStr)
	{
		return GetTransactions()
		.Where(tuple => TryParseBitcoinAddress(tuple.Item1.WalletModel.Network, queryStr, out var address) ?
			ContainsDestinationAddress(tuple.Item1, tuple.Item2, address) :
			ContainsId(tuple.Item2, queryStr));
	}

	private static bool ContainsDestinationAddress(WalletViewModel walletViewModel, HistoryItemViewModelBase historyItem, BitcoinAddress address)
	{
		var txid = historyItem.Transaction.Id;
		return walletViewModel.WalletModel.Transactions.GetDestinationAddresses(txid).Contains(address);
	}

	private bool TryParseBitcoinAddress(Network network, string queryStr, [NotNullWhen(true)] out BitcoinAddress? address)
	{
		address = null;
		try
		{
			address = BitcoinAddress.Create(queryStr, network);
			return true;
		}
		catch (FormatException)
		{
			return false;
		}
	}
}
