using System.Collections.Generic;
using System.Linq;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Send;

public partial class TransactionSummaryViewModel : ViewModelBase
{
	private readonly Network _network;
	[AutoNotify] private bool _transactionHasChange;
	[AutoNotify] private bool _isOtherPocketSelectionPossible;
	[AutoNotify] private LabelsArray _labels = LabelsArray.Empty;
	[AutoNotify] private LabelsArray _recipient = LabelsArray.Empty;
	[AutoNotify] private Amount? _fee;
	[AutoNotify] private Amount? _amount;
	[AutoNotify] private double? _amountDiff;
	[AutoNotify] private double? _feeDiff;
	[AutoNotify] private IReadOnlyList<RecipientSummaryViewModel> _recipients = Array.Empty<RecipientSummaryViewModel>();

	public TransactionSummaryViewModel(UiContext uiContext, TransactionPreviewViewModel parent, IWalletModel wallet, TransactionInfo info, bool isPreview = false) : base(uiContext)
	{
		Parent = parent;
		_network = wallet.Network;
		IsPreview = isPreview;
		IsPayToMany = info.IsPayToMany;
		AddressText = info.Destination.ToString(_network);
		PayJoinUrl = info.PayJoinClient?.PaymentUrl.AbsoluteUri;
		IsPayJoin = PayJoinUrl is not null;
	}

	public TransactionPreviewViewModel Parent { get; }

	public bool IsPreview { get; }

	public string AddressText { get; }

	public string? PayJoinUrl { get; }

	public bool IsPayJoin { get; }

	public bool IsPayToMany { get; }

	public void UpdateTransaction(BuildTransactionResult transactionResult, TransactionInfo info)
	{
		Money destinationAmount;
		if (info.IsPayToMany)
		{
			var fee = transactionResult.Fee;
			var hasSubtractFee = info.AllRecipients.Any(r => r.IsSubtractFee);

			// For pay-to-many, show per-recipient amounts. If a recipient used "Max" (SubtractFee),
			// display their actual received amount (requested minus fee) instead of the raw request.
			Recipients = info.AllRecipients.Select(r =>
			{
				var displayAmount = r.IsSubtractFee ? r.Amount - fee : r.Amount;
				return new RecipientSummaryViewModel(
					UiContext,
					r.Destination.ToString(_network),
					UiContext.AmountProvider.Create(displayAmount),
					r.Label);
			}).ToList();

			// Total destination amount: subtract fee only if one of the recipients absorbs it.
			destinationAmount = hasSubtractFee
				? info.TotalAmount - fee
				: info.TotalAmount;
		}
		else
		{
			destinationAmount = transactionResult.CalculateDestinationAmount(info.Destination);
		}

		Amount = UiContext.AmountProvider.Create(destinationAmount);
		Fee = UiContext.AmountProvider.Create(transactionResult.Fee);

		Recipient = info.Recipient;
		IsOtherPocketSelectionPossible = info.IsOtherPocketSelectionPossible;
		AmountDiff = DiffOrNull(Amount, Parent.CurrentTransactionSummary.Amount);
		FeeDiff = DiffOrNull(Fee, Parent.CurrentTransactionSummary.Fee);
	}

	private static double? DiffOrNull(Amount? current, Amount? previous)
	{
		if (current is null || previous is null)
		{
			return null;
		}

		return current.Diff(previous);
	}
}
