using Avalonia.Controls;
using Avalonia.Markup.Xaml;

namespace MagicalCryptoWallet.Fluent.Views.Wallets.Receive;

public class ReceiveAddressesView : UserControl
{
	public ReceiveAddressesView()
	{
		InitializeComponent();
	}

	private void InitializeComponent()
	{
		AvaloniaXamlLoader.Load(this);
	}
}
