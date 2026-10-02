using System.Collections.Generic;
using System.Collections.Immutable;
using System.Linq;
using NBitcoin;
using ReactiveUI;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Fluent.Models.Transactions;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Send;

public partial class TransactionInfo
{
	[AutoNotify] private FeeRate _feeRate = FeeRate.Zero;
	[AutoNotify] private IEnumerable<SmartCoin> _coins = Enumerable.Empty<SmartCoin>();

	public TransactionInfo(Destination destination, int anonScoreTarget)
	{
		Destination = destination;
		PrivateCoinThreshold = anonScoreTarget;

		this.WhenAnyValue(x => x.FeeRate)
			.Subscribe(_ => OnFeeChanged());

		this.WhenAnyValue(x => x.Coins)
			.Subscribe(_ => OnCoinsChanged());
	}

	public int PrivateCoinThreshold { get; }

	public Money Amount { get; init; } = Money.Zero;

	public Destination Destination { get; init; }

	public LabelsArray Recipient { get; set; } = LabelsArray.Empty;

	public IEnumerable<SmartCoin> ChangelessCoins { get; set; } = Enumerable.Empty<SmartCoin>();

	public bool IsOptimized => ChangelessCoins.Any();

	public bool SubtractFee { get; init; }

	public bool IsOtherPocketSelectionPossible { get; set; }

	public bool IsFixedAmount { get; init; }

	public IReadOnlyList<RecipientInfo> AdditionalRecipients { get; init; } = ImmutableList<RecipientInfo>.Empty;

	public bool IsPayToMany => AdditionalRecipients.Count > 0;

	public IEnumerable<RecipientInfo> AllRecipients
	{
		get
		{
			yield return new RecipientInfo(Destination, Amount, Recipient, IsSubtractFee: SubtractFee);
			foreach (var r in AdditionalRecipients)
			{
				yield return r;
			}
		}
	}

	public Money TotalAmount => AllRecipients.Aggregate(Money.Zero, (sum, r) => sum + r.Amount);

	private void OnFeeChanged()
	{
		ChangelessCoins = Enumerable.Empty<SmartCoin>();
	}

	private void OnCoinsChanged()
	{
		ChangelessCoins = Enumerable.Empty<SmartCoin>(); // Clear ChangelessCoins on pocket change, so we calculate the suggestions with the new pocket.
	}

	public TransactionInfo Clone()
	{
		return new TransactionInfo(Destination, PrivateCoinThreshold)
		{
			FeeRate = FeeRate,
			Coins = Coins,
			Amount = Amount,
			Destination = Destination,
			Recipient = Recipient,
			ChangelessCoins = ChangelessCoins,
			SubtractFee = SubtractFee,
			IsOtherPocketSelectionPossible = IsOtherPocketSelectionPossible,
			IsFixedAmount = IsFixedAmount,
			AdditionalRecipients = AdditionalRecipients
		};
	}
}
