using System;
using Avalonia.Controls;
using Avalonia.Markup.Xaml;
using MagicalCryptoWallet.Fluent.Controls;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.ReleaseHighlights;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.Fluent.Views.Dialogs.ReleaseHighlights;

public class ReleaseHighlightsDialogView : UserControl
{
	public ReleaseHighlightsDialogView()
	{
		InitializeComponent();
		this.FindControl<ReleaseHighlightsText>("ReleaseNotes")!.LinkClicked += OnLinkClicked;
	}
	private async void OnLinkClicked(object? sender, string target)
	{
		if (DataContext is not ReleaseHighlightsDialogViewModel viewModel) { return; }
		try { await viewModel.UiContext.OpenBrowserAsync(target); }
		catch (Exception) { Logger.LogWarning("Could not open release-highlights link."); }
	}
	private void InitializeComponent()
	{
		AvaloniaXamlLoader.Load(this);
	}
}
