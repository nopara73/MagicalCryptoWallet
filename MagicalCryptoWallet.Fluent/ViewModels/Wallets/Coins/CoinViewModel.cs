using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using ReactiveUI;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Coins;

public class CoinViewModel : CoinListItem
{
	public CoinViewModel(UiContext uiContext, LabelsArray labels, CoinModel coin) : base(uiContext)
	{
		Labels = labels;
		Coin = coin;
		BtcAddress = coin.BtcAddress;
		Amount = new Amount(coin.Amount);
		IsConfirmed = coin.IsConfirmed;
		IsBanned = coin.IsBanned;
		var confirmationCount = coin.Confirmations;
		ConfirmationStatus = $"{confirmationCount} confirmation{TextHelpers.AddSIfPlural((int)confirmationCount)}";
		BannedUntilUtcToolTip = coin.BannedUntilUtcToolTip;
		AnonymityScore = coin.AnonScore;
		BannedUntilUtc = coin.BannedUntilUtc;
		ScriptType = coin.ScriptType;
		this.WhenAnyValue(x => x.Coin.IsCoinJoinInProgress).BindTo(this, x => x.IsCoinjoining).DisposeWith(_disposables);
	}

	public CoinModel Coin { get; }
	public override string Key => Coin.Key.ToString();
}
