using Avalonia.Controls;
using Avalonia.Markup.Xaml;

namespace MagicalCryptoWallet.Fluent.Views.Settings;

public class CoordinatorTabSettingsView : UserControl
{
	public CoordinatorTabSettingsView()
	{
		InitializeComponent();
	}

	private void InitializeComponent()
	{
		AvaloniaXamlLoader.Load(this);
	}
}
