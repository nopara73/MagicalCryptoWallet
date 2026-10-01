using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Fluent.Models.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Send;

public class RecipientSummaryViewModel : ViewModelBase
{
	public RecipientSummaryViewModel(UiContext uiContext, string addressText, Amount amount, LabelsArray recipient) : base(uiContext)
	{
		AddressText = addressText;
		Amount = amount;
		Recipient = recipient;
	}

	public string AddressText { get; }

	public Amount Amount { get; }

	public LabelsArray Recipient { get; }
}
