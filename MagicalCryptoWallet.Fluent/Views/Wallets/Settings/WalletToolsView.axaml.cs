using Avalonia.Controls;
using Avalonia.Markup.Xaml;

namespace MagicalCryptoWallet.Fluent.Views.Wallets.Settings;

public class WalletToolsView : UserControl
{
	public WalletToolsView()
	{
		InitializeComponent();
	}

	private void InitializeComponent()
	{
		AvaloniaXamlLoader.Load(this);
	}
}
