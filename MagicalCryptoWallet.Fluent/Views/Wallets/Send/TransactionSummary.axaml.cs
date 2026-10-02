using Avalonia.Controls;
using Avalonia.Markup.Xaml;

namespace MagicalCryptoWallet.Fluent.Views.Wallets.Send;

public class TransactionSummary : UserControl
{
	public TransactionSummary()
	{
		InitializeComponent();
	}

	private void InitializeComponent()
	{
		AvaloniaXamlLoader.Load(this);
	}
}
