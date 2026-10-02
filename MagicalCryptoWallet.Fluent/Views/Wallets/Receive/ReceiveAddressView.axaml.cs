using Avalonia.Controls;
using Avalonia.Markup.Xaml;

namespace MagicalCryptoWallet.Fluent.Views.Wallets.Receive;

public class ReceiveAddressView : UserControl
{
	public ReceiveAddressView()
	{
		InitializeComponent();
	}

	private void InitializeComponent()
	{
		AvaloniaXamlLoader.Load(this);
	}
}
