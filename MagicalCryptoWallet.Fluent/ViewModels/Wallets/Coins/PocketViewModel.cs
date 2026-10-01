using System.Collections.Generic;
using System.Linq;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Coins;

public class PocketViewModel : CoinListItem
{
	public PocketViewModel(UiContext uiContext, Pocket pocket, ICoinListModel availableCoins) : base(uiContext)
	{
		var pocketCoins = pocket.Coins.ToList();

		var unconfirmedCount = pocketCoins.Count(x => !x.Confirmed);
		IsConfirmed = unconfirmedCount == 0;
		ConfirmationStatus = IsConfirmed ? "All coins are confirmed" : $"{unconfirmedCount} coins are waiting for confirmation";
		IsBanned = pocketCoins.Any(x => x.IsBanned);
		BannedUntilUtcToolTip = IsBanned ? "Some coins can't participate in coinjoin" : null;
		Amount = new Amount(pocket.Amount);
		IsCoinjoining = pocketCoins.Any(x => x.CoinJoinInProgress);
		AnonymityScore = GetAnonScore(pocketCoins);
		Labels = pocket.Labels;
		Children =
			pocketCoins
				.Select(availableCoins.GetCoinModel)
				.OrderByDescending(x => x.AnonScore)
				.Select(coin => new CoinViewModel(uiContext, "", coin) { IsChild = true })
				.ToList();

		Children
			.AsObservableChangeSet()
			.AutoRefresh(x => IsCoinjoining)
			.Select(_ => Children.Any(x => x.IsCoinjoining))
			.BindTo(this, x => x.IsCoinjoining)
			.DisposeWith(_disposables);

		ScriptType = null;
		foreach (var child in Children)
		{
			_disposables.Add(child);
		}
	}

	private static int? GetAnonScore(IEnumerable<SmartCoin> pocketCoins)
	{
		var allScores = pocketCoins.Select(x => (int?)x.AnonymitySet);
		return CommonOrDefault(allScores.ToList());
	}

	/// <summary>
	/// Returns the common item in the list, if any.
	/// </summary>
	/// <typeparam name="T">Type of the item</typeparam>
	/// <param name="list">List of items to determine the common item.</param>
	/// <returns>The common item or <c>default</c> if there is no common item.</returns>
	private static T? CommonOrDefault<T>(IList<T> list)
	{
		var commonItem = list[0];

		for (var i = 1; i < list.Count; i++)
		{
			if (!Equals(list[i], commonItem))
			{
				return default;
			}
		}

		return commonItem;
	}

	public override string Key => Labels.ToString();
}
