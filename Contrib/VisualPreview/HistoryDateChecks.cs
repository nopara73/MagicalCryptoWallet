using System.ComponentModel;
using System.Globalization;
using System.Reflection;
using System.Runtime.CompilerServices;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Automation.Peers;
using Avalonia.Controls.Models.TreeDataGrid;
using Avalonia.Controls.Primitives;
using Avalonia.Headless;
using Avalonia.Media.Imaging;
using Avalonia.Styling;
using Avalonia.Threading;
using Avalonia.VisualTree;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Controls;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.TreeDataGrid;
using MagicalCryptoWallet.Fluent.ViewModels;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.History;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.History.Details;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.History.HistoryItems;
using MagicalCryptoWallet.Fluent.Views.Wallets.Home.History;
using MagicalCryptoWallet.Fluent.Views.Wallets.Home.History.Details;

internal static class HistoryDateChecks
{
	public static void Run(UiContext context, string destination)
	{
		var config = new UiConfig(Path.Combine(Path.GetFullPath(destination), "synthetic-ui-config.json"));
		var services = (Services)RuntimeHelpers.GetUninitializedObject(typeof(Services));
		SetBackingField(services, nameof(Services.UiConfig), config);
		typeof(Services).GetProperty(nameof(Services.Instance))!.SetValue(null, services);
		var originalCulture = CultureInfo.CurrentCulture;
		var originalUiCulture = CultureInfo.CurrentUICulture;
		try
		{
			// Run with DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=0 to cover regional calendars too.
			foreach (var cultureName in new[] { "en-US", "en-GB", "de-DE", "hu-HU", "ar-SA", "fa-IR", "th-TH" })
			{
				CultureInfo.CurrentCulture = CultureInfo.GetCultureInfo(cultureName);
				CultureInfo.CurrentUICulture = CultureInfo.CurrentCulture;
				foreach (var (first, last) in new[]
				{
					(LocalDate(2026, 10, 2, 0, 1), LocalDate(2026, 10, 2, 23, 59)),
					(LocalDate(2026, 12, 31, 23, 59), LocalDate(2027, 1, 1, 0, 0)),
					(LocalDate(2026, 9, 2, 12, 0), LocalDate(2026, 10, 2, 12, 0))
				})
				{
					CheckGroup(first, last);
				}
				foreach (var theme in new[] { ThemeVariant.Light, ThemeVariant.Dark })
				{
					Application.Current!.RequestedThemeVariant = theme;
					CheckViews(context, config, cultureName, theme, destination);
				}
			}
		}
		finally
		{
			CultureInfo.CurrentCulture = originalCulture;
			CultureInfo.CurrentUICulture = originalUiCulture;
			config.PrivacyMode = false;
		}
		Console.WriteLine("History date checks passed in seven locales and both themes: full local timestamps, grouped date ranges, tooltips, details, sorting, unchanged amount rendering, and privacy masking.");
	}

	private static void CheckGroup(DateTimeOffset first, DateTimeOffset last)
	{
		var children = new[] { Coinjoin(10, first), Coinjoin(11, last) };
		var amounts = children.Select(x => x.Amount).ToArray();
		var builder = (TransactionTreeBuilder)RuntimeHelpers.GetUninitializedObject(typeof(TransactionTreeBuilder));
		var group = (CoinJoinTransactionGroupModel)typeof(TransactionTreeBuilder)
			.GetMethod("CreateCoinjoinGroup", BindingFlags.Instance | BindingFlags.NonPublic)!.Invoke(builder, [children])!;
		Check(group.Date == last && group.DateString == Expected(last), "Grouped rows must retain the latest full local timestamp.");
		Check(group.DateToolTipString == $"{Expected(first)} - {Expected(last)}", "Grouped tooltips must retain both full timestamp endpoints.");
		Check(group.Amount == Money.Satoshis(amounts.Sum(amount => amount.Satoshi)), "Date formatting must preserve the grouped amount.");
		for (var i = 0; i < children.Length; i++)
		{
			Check(children[i].IsChild && children[i].DateString == Expected(children[i].Date), "Grouped children must keep their full date and time.");
			Check(children[i].Amount == amounts[i], "Date formatting must preserve each child amount.");
		}
	}

	private static void CheckViews(UiContext context, UiConfig config, string cultureName, ThemeVariant theme, string destination)
	{
		var wallet = DispatchProxy.Create<IWalletModel, InertPreviewWallet>();
		var history = NewModel<HistoryViewModel>(context);
		var models = new[]
		{
			Regular(1, LocalDate(2026, 12, 31, 23, 59), Money.Satoshis(123_456), TransactionType.IncomingTransaction),
			Regular(2, LocalDate(2027, 1, 1, 0, 0), Money.Satoshis(-123_456), TransactionType.OutgoingTransaction)
		};
		var rows = models.Select(model => new TransactionHistoryItemViewModel(context, wallet, model)).Cast<HistoryItemViewModelBase>().ToArray();
		using var source = new HierarchicalTreeDataGridSource<HistoryItemViewModelBase>(rows);
		foreach (var columnName in new[] { "IndicatorsColumn", "DateColumn", "AmountColumn", "LabelsColumn" })
		{
			source.Columns.Add((IColumn<HistoryItemViewModelBase>)typeof(HistoryViewModel)
				.GetMethod(columnName, BindingFlags.Static | BindingFlags.NonPublic)!.Invoke(null, null)!);
		}
		typeof(HistoryViewModel).GetProperty(nameof(HistoryViewModel.Source))!.SetValue(history, source);
		var window = new Window { Width = 900, Height = 320, Content = new HistoryTable { DataContext = history } };
		config.PrivacyMode = false;
		window.Show();
		Flush();
		var dateCells = window.GetVisualDescendants().OfType<TreeDataGridDatePrivacyTextCell>().ToArray();
		Check(dateCells.Length == models.Length, "The actual history must render both synthetic transactions.");
		foreach (var cell in dateCells)
		{
			var row = (HistoryItemViewModelBase)cell.DataContext!;
			Check(Equals(ToolTip.GetTip(cell), Expected(row.Transaction.Date)), "History tooltips must use the full ISO timestamp.");
			Check(Equals(Field(cell, "_text"), Expected(row.Transaction.Date)), "The visible date column must use the full ISO timestamp.");
		}
		var amountsBefore = AmountTexts(window);
		Check(amountsBefore.Length >= 2, "The history must render its existing incoming and outgoing amount controls.");
		source.SortBy(source.Columns[1], ListSortDirection.Descending);
		Flush();
		Check(((HistoryItemViewModelBase)source.Rows[0].Model!).Transaction.Id == models[1].Id, "Date sorting must continue to use timestamp values.");
		source.SortBy(source.Columns[1], ListSortDirection.Ascending);
		Flush();
		Check(((HistoryItemViewModelBase)source.Rows[0].Model!).Transaction.Id == models[0].Id, "Ascending date sorting must preserve chronological order.");
		Check(AmountTexts(window).SequenceEqual(amountsBefore), "Date sorting must preserve amount text.");
		Save(window, Path.Combine(destination, $"history-dates-{cultureName}-{theme}.png"));
		config.PrivacyMode = true;
		Flush();
		Check(dateCells.All(cell => Equals(Field(cell, "_isContentVisible"), false)), "Privacy mode must still mask history dates.");
		Save(window, Path.Combine(destination, $"history-dates-{cultureName}-{theme}-private.png"));
		config.PrivacyMode = false;
		Flush();
		Check(AmountTexts(window).SequenceEqual(amountsBefore), "Revealing dates must preserve amount text.");
		window.Close();

		// Render the actual details bindings with the model's presentation values.
		var details = NewModel<TransactionDetailsViewModel>(context);
		details.DateString = models[0].DateToolTipString;
		details.Amount = new Amount(models[0].Amount);
		details.AmountText = "Amount received";
		SetBackingField(details, nameof(details.TransactionId), uint256.One);
		SetBackingField(details, nameof(details.TransactionHex), "synthetic");
		SetBackingField(details, nameof(details.DestinationAddresses), Array.Empty<BitcoinAddress>());
		var detailsWindow = new Window { Width = 900, Height = 600, Content = new TransactionDetailsView { DataContext = details } };
		detailsWindow.Show();
		Flush();
		var dateItem = detailsWindow.GetVisualDescendants().OfType<PreviewItem>().Single(item => item.Label == "Date / Time");
		Check(Equals(dateItem.CopyableContent, Expected(models[0].Date)), "Transaction details must copy the full local timestamp.");
		Check(dateItem.GetVisualDescendants().OfType<TextBlock>().Any(text => text.Text == Expected(models[0].Date)), "Transaction details must visibly show the same timestamp.");
		Save(detailsWindow, Path.Combine(destination, $"history-details-{cultureName}-{theme}.png"));
		detailsWindow.Close();
	}

	private static RegularTransactionModel Regular(ulong id, DateTimeOffset date, Money amount, TransactionType type)
	{
		var model = (RegularTransactionModel)Activator.CreateInstance(typeof(RegularTransactionModel), [type])!;
		Initialize(model, id, date, amount);
		return model;
	}

	private static CoinJoinTransactionModel Coinjoin(ulong id, DateTimeOffset date)
	{
		var model = Activator.CreateInstance<CoinJoinTransactionModel>();
		Initialize(model, id, date, Money.Satoshis(-100));
		return model;
	}

	private static void Initialize(SingleTransactionModel model, ulong id, DateTimeOffset date, Money amount)
	{
		Set(nameof(model.Id), new uint256(id));
		Set(nameof(model.OrderIndex), (int)id);
		Set(nameof(model.Labels), LabelsArray.Empty);
		Set(nameof(model.Date), date);
		Set(nameof(model.DateString), date.ToUserFacingString());
		Set(nameof(model.DateToolTipString), date.ToUserFacingString());
		Set(nameof(model.Amount), amount);
		Set(nameof(model.Status), TransactionStatus.Confirmed);
		Set(nameof(model.Confirmations), 1u);
		Set(nameof(model.ConfirmedTooltip), "Confirmed");
		Set(nameof(model.HexFunction), (Func<string>)(() => "synthetic"));
		// Supply empty inputs and outputs when the transaction model exposes them.
		Set("ForeignInputsFunction", (Func<IReadOnlyCollection<OutPoint>>)(() => Array.Empty<OutPoint>()));
		Set("ForeignOutputsFunction", (Func<IReadOnlyCollection<IndexedTxOut>>)(() => Array.Empty<IndexedTxOut>()));
		Set("WalletInputs", Array.Empty<SmartCoin>());
		Set("WalletOutputs", Array.Empty<SmartCoin>());
		void Set(string name, object value) => model.GetType().GetProperty(name)?.SetValue(model, value);
	}

	private static string[] AmountTexts(Window window) => window.GetVisualDescendants().OfType<AmountControl>()
		.SelectMany(control => control.GetVisualDescendants().OfType<TextBlock>())
		.Where(text => text.IsEffectivelyVisible).Select(text => ControlAutomationPeer.CreatePeerForElement(text).GetName() ?? "").Order(StringComparer.Ordinal).ToArray();

	private static DateTimeOffset LocalDate(int year, int month, int day, int hour, int minute) => new(new DateTime(year, month, day, hour, minute, 0, DateTimeKind.Local));
	private static string Expected(DateTimeOffset date) => date.LocalDateTime.ToString("yyyy-MM-dd HH:mm", CultureInfo.InvariantCulture);
	private static T NewModel<T>(UiContext context) where T : ViewModelBase
	{
		var model = (T)RuntimeHelpers.GetUninitializedObject(typeof(T));
		typeof(ViewModelBase).GetConstructor([typeof(UiContext)])!.Invoke(model, [context]);
		return model;
	}
	private static void SetBackingField(object target, string name, object value) => target.GetType()
		.GetField($"<{name}>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
	private static object? Field(object target, string name) => typeof(TreeDataGridPrivacyTextCell).GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.GetValue(target);
	private static void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
	private static void Flush() { Dispatcher.UIThread.RunJobs(); AvaloniaHeadlessPlatform.ForceRenderTimerTick(); Dispatcher.UIThread.RunJobs(); }
	private static void Save(Window window, string path)
	{
		using var bitmap = new RenderTargetBitmap(new PixelSize((int)window.Width, (int)window.Height));
		bitmap.Render(window);
		bitmap.Save(path);
	}
}
