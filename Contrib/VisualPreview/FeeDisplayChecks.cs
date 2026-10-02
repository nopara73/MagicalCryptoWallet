using System.Collections.ObjectModel;
using System.Reflection;
using System.Runtime.CompilerServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Styling;
using Avalonia.Threading;
using Avalonia.VisualTree;
using NBitcoin;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Controls;
using MagicalCryptoWallet.Fluent.Models;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Coinjoins;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.History.Details;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.History.Features;
using MagicalCryptoWallet.Fluent.Views.Wallets.Home.History.Details;
using MagicalCryptoWallet.Fluent.Views.Wallets.Home.History.Features;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.WabiSabi.Client;
using static AutomaticCoinSelectionChecks;

internal static class FeeDisplayChecks
{
	public static void Run(UiContext context, string destination)
	{
		Services.Instance.UiConfig.PrivacyMode = false;
		foreach (var theme in new[] { ThemeVariant.Light, ThemeVariant.Dark })
		{
			Application.Current!.RequestedThemeVariant = theme;
			foreach (var rate in new[] { 100_000m, 0m })
			{
				var services = DispatchProxy.Create<IServices, AutomaticPreviewServices>();
				((AutomaticPreviewServices)(object)services).UsdExchangeRate = rate;
				var provider = new AmountProvider(services);
				foreach (var (name, view) in CreateViews(context, provider, rate))
				{
					var window = new Window
					{
						Width = 900, Height = 760, Content = view,
						Background = theme == ThemeVariant.Dark ? new SolidColorBrush(Color.Parse("#151515")) : Brushes.White
					};
					window.Show();
					try
					{
						Flush();
						window.Measure(new Size(900, 760));
						window.Arrange(new Rect(0, 0, 900, 760));
						Flush();
						var fee = window.GetVisualDescendants().OfType<PreviewItem>().SingleOrDefault(item => item.Label == "Total fee");
						var feeContent = fee is null ? view : (Control)fee;
						Check(!feeContent.GetVisualDescendants().OfType<AmountControl>().Any(), $"{name} must display its fee without BTC.");
						Check(!window.GetVisualDescendants().OfType<TextBlock>().Any(text => text.Text is "Fee Rate" or "Expected confirmation time" or "Mining fee" or "Wasted dust"), $"{name} must have no fee breakdown, rate, or confirmation estimate.");
						var expected = rate > 0 ? "0.28 USD" : "—";
						Check(feeContent.GetVisualDescendants().OfType<TextBlock>().Any(text => text.Text == expected && text.IsEffectivelyVisible), $"{name} must show {expected}.");
						if (fee?.CopyableContent is not null)
						{
							Check(rate > 0 ? Equals(fee.CopyableContent, expected) : !fee.IsCopyButtonEnabled, $"{name} must copy only an available USD fee.");
						}
						foreach (var scale in new[] { 1.0, 2.0 })
						{
							using var bitmap = new RenderTargetBitmap(new PixelSize((int)(900 * scale), (int)(760 * scale)), new Vector(96 * scale, 96 * scale));
							bitmap.Render(window);
							bitmap.Save(Path.Combine(destination, $"{name}-{theme}-{scale * 100:0}-{(rate > 0 ? "usd" : "no-quote")}.png"));
						}
						if (rate > 0 && name != "transaction-preview")
						{
							services.EventBus.Publish(new ExchangeRateChanged(200_000m));
							Flush();
							Check(feeContent.GetVisualDescendants().OfType<TextBlock>().Any(text => text.Text == "0.56 USD" && text.IsEffectivelyVisible), $"{name} must update with the USD quote.");
							services.EventBus.Publish(new ExchangeRateChanged(rate));
							Flush();
						}
					}
					finally { window.Close(); }
				}
			}
		}
		Console.WriteLine("Fee display checks passed: send, history, CoinJoin, speed-up, and cancellation show USD-only totals, handle missing and changing quotes, and omit confirmation estimates.");
	}

	private static IEnumerable<(string Name, Control View)> CreateViews(UiContext context, AmountProvider provider, decimal rate)
	{
		var amount = provider.Create(Money.Coins(0.01m));
		var fee = provider.Create(Money.Satoshis(280));
		var costs = new CoinjoinCostsViewModel(provider.Create);
		costs.Update(new CoinjoinCosts(Money.Satoshis(200), Money.Satoshis(80), Money.Zero), Money.Satoshis(-280));
		yield return ("transaction-preview", CreateTransactionPreview(context, usdExchangeRate: rate));

		var regular = NewModel<TransactionDetailsViewModel>(context);
		regular.AmountText = "Amount sent";
		regular.Amount = amount;
		regular.DateString = "2026-10-02 12:00";
		SetBackingField(regular, nameof(regular.Fee), fee);
		SetBackingField(regular, nameof(regular.IsFeeVisible), true);
		SetBackingField(regular, nameof(regular.TransactionId), uint256.One);
		SetBackingField(regular, nameof(regular.TransactionHex), "synthetic");
		SetBackingField(regular, nameof(regular.DestinationAddresses), Array.Empty<BitcoinAddress>());
		yield return ("transaction-details", new TransactionDetailsView { DataContext = regular });

		var coinjoin = NewModel<CoinJoinDetailsViewModel>(context);
		coinjoin.Date = "2026-10-02 12:00";
		coinjoin.TransactionId = uint256.One;
		SetBackingField(coinjoin, nameof(coinjoin.Costs), costs);
		SetBackingField(coinjoin, nameof(coinjoin.TransactionHex), "synthetic");
		yield return ("coinjoin-details", new CoinJoinDetailsView { DataContext = coinjoin });

		var group = NewModel<CoinJoinsDetailsViewModel>(context);
		group.Date = "2026-10-02";
		group.Status = "Pending";
		group.TransactionIds = new ObservableCollection<uint256> { uint256.One };
		group.TxCount = 1;
		SetBackingField(group, nameof(group.Costs), costs);
		yield return ("coinjoins-details", new CoinJoinsDetailsView { DataContext = group });

		var cancel = NewModel<CancelTransactionDialogViewModel>(context);
		SetBackingField(cancel, nameof(cancel.Fee), fee);
		yield return ("cancellation-fee", new CancelTransactionDialogView { DataContext = cancel });

		foreach (var paying in new[] { true, false })
		{
			var speedup = NewModel<SpeedUpTransactionDialogViewModel>(context);
			SetBackingField(speedup, nameof(speedup.Fee), fee);
			SetBackingField(speedup, nameof(speedup.AreWePayingTheFee), paying);
			yield return (paying ? "speedup-fee" : "speedup-recipient-fee", new SpeedUpTransactionDialogView { DataContext = speedup });
		}
	}

	private static void Flush()
	{
		Dispatcher.UIThread.RunJobs();
		AvaloniaHeadlessPlatform.ForceRenderTimerTick();
		Dispatcher.UIThread.RunJobs();
	}
	private static void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
}
