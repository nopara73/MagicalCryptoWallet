using System.Diagnostics.CodeAnalysis;
using System.Reflection;
using System.Reactive.Linq;
using System.Runtime.CompilerServices;
using Avalonia;
using Avalonia.Automation.Peers;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Input;
using Avalonia.Threading;
using Avalonia.VisualTree;
using DynamicData;
using NBitcoin;
using ReactiveUI;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Controls;
using MagicalCryptoWallet.Fluent.Models;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Advanced;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Coins;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Send;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Settings;
using MagicalCryptoWallet.Fluent.Views.Wallets;
using MagicalCryptoWallet.Fluent.Views.Wallets.Advanced;
using MagicalCryptoWallet.Fluent.Views.Wallets.Send;
using MagicalCryptoWallet.Fluent.Views.Wallets.Settings;
using MagicalCryptoWallet.Fluent.Views.Shell;
using MagicalCryptoWallet.Models;
using Key = Avalonia.Input.Key;

internal static class AutomaticCoinSelectionChecks
{
	public static void Run(UiContext context)
	{
		int sends = 0, confirmations = 0;
		CheckView(CreateWalletActions(context, () => sends++), window =>
		{
			var send = window.GetVisualDescendants().OfType<Button>().Single(b => ControlAutomationPeer.CreatePeerForElement(b).GetName() == "Send");
			Check(send.Flyout is null && !window.GetVisualDescendants().OfType<SubActionButton>().Any(b => ReferenceEquals(b.Command, send.Command)), "Send must be one action without an input-selection menu.");
			Click(window, send);
			Check(sends == 1, "The real Send button must activate exactly once.");
		});
		CheckView(CreateTransactionPreview(context, () => confirmations++), window =>
		{
			window.KeyPress(Key.LeftAlt, RawInputModifiers.Alt, PhysicalKey.AltLeft, "");
			Flush();
			Check(!window.GetVisualDescendants().OfType<Button>().Any(b => Equals(b.Content, "Review coins")), "Alt must not reveal input selection.");
			window.KeyRelease(Key.LeftAlt, RawInputModifiers.None, PhysicalKey.AltLeft, "");
			Check(!window.GetVisualDescendants().OfType<Slider>().Any(), "Transaction fees must have no slider.");
			Check(!window.GetVisualDescendants().OfType<Button>().Any(b => Equals(ToolTip.GetTip(b), "Change transaction fee or confirmation time")), "Transaction fees must have no adjustment button.");
			Check(!window.GetVisualDescendants().OfType<TextBlock>().Any(t => t.Text is "Fee Rate" or "Expected confirmation time"), "The send preview must omit fee rate and expected confirmation time.");
			var fee = window.GetVisualDescendants().OfType<PreviewItem>().Single(item => item.Label == "Total fee");
			Check(!fee.GetVisualDescendants().OfType<AmountControl>().Any(), "The fee must not display a BTC amount.");
			Check(fee.GetVisualDescendants().OfType<TextBlock>().Any(t => t.Text == "0.28 USD" && t.IsEffectivelyVisible), "The total fee must display only its USD value to two decimals.");
			Check(Equals(fee.CopyableContent, "0.28 USD"), "Copying the fee must copy its displayed USD value.");
			var confirm = window.GetVisualDescendants().OfType<Button>().Single(b => Equals(b.Content, "Confirm"));
			Click(window, confirm);
			Check(confirmations == 1, "Transaction confirmation must remain operable.");
		});
		CheckView(CreateTransactionPreview(context, usdExchangeRate: null), window =>
		{
			var fee = window.GetVisualDescendants().OfType<PreviewItem>().Single(item => item.Label == "Total fee");
			Check(fee.GetVisualDescendants().OfType<TextBlock>().Any(t => t.Text == "—" && t.IsEffectivelyVisible), "A missing USD quote must show an unavailable value.");
			Check(!fee.IsCopyButtonEnabled, "A missing USD quote must not copy a zero-dollar fee.");
			Check(!fee.GetVisualDescendants().OfType<TextBlock>().Any(t => t.Text == "0.00 USD" && t.IsEffectivelyVisible), "A missing USD quote must not be displayed as a free transaction.");
		});
		CheckView(CreateWalletCoins(context), window =>
		{
			Check(!window.GetVisualDescendants().OfType<CheckBox>().Any(), "Coin details must have no spend-selection checkboxes.");
			var grid = window.GetVisualDescendants().OfType<TreeDataGrid>().Single();
			var model = (CoinListViewModel)grid.DataContext!;
			Check(model.CoinItems.Count == 2 && model.TreeDataGridSource.Columns.Count == 4, "Read-only coin details must retain status, privacy, amounts and labels.");
			model.TreeDataGridSource.SortBy(model.TreeDataGridSource.Columns[2], System.ComponentModel.ListSortDirection.Ascending);
			Check(model.TreeDataGridSource.Rows.Count >= 2, "Expanding and sorting the read-only list must remain functional.");
		});
		CheckView(CreateWalletSettings(context), window =>
		{
			Check(!window.GetVisualDescendants().OfType<TextBlock>().Any(t => t.Text == "Default Send Workflow"), "Settings must not offer a manual send workflow.");
			Check(window.GetVisualDescendants().OfType<ComboBox>().Count(c => c.IsVisible) == 2, "Receive and change address-type settings must remain available.");
		});
		Console.WriteLine("Automatic send UI checks passed: USD-only total fee and clipboard, no fee rate or expected confirmation time, missing-quote handling, working confirmation, automatic inputs, and read-only coin details.");
	}

	[SuppressMessage("Reliability", "CA2000:Dispose objects before losing scope", Justification = "Commands are disposed when the returned view detaches.")]
	public static Control CreateWalletActions(UiContext context, Action? send = null, string status = "Ready", bool hasCachedData = true)
	{
		var model = NewModel<WalletViewModel>(context);
		SetProperty(model, nameof(WalletViewModel.Title), "Magical Crypto Wallet");
		SetProperty(model, nameof(WalletViewModel.IsSendButtonVisible), true);
		SetProperty(model, nameof(WalletViewModel.HasCachedData), hasCachedData);
		model.CanSpend = status == "Ready" && hasCachedData;
		SetBackingField(model, nameof(WalletViewModel.WalletModel), NewWallet());
		SetBackingField(model, nameof(WalletViewModel.Settings), NewModel<WalletSettingsViewModel>(context));
		SetBackingField(model, nameof(WalletViewModel.Tiles), new ActivatableViewModel[]
		{
			new MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.Tiles.WalletBalanceTileViewModel(context, Observable.Return(new Amount(Money.Coins(0.055m))))
		});
		var command = ReactiveCommand.Create(() => send?.Invoke(), Observable.Return(model.CanSpend));
		var receiveCommand = ReactiveCommand.Create(() => { }, Observable.Return(hasCachedData));
		SetProperty(model, nameof(WalletViewModel.SendCommand), command);
		SetProperty(model, nameof(WalletViewModel.SegwitReceiveCommand), receiveCommand);
		model.DefaultReceiveCommand = receiveCommand;
		var sync = NewModel<WalletSyncStatusViewModel>(context);
		sync.StatusText = status;
		sync.IsExpanded = status is "Syncing" or "Faulted";
		sync.IsFaulted = status == "Faulted";
		sync.Error = sync.IsFaulted ? "Synthetic storage is unavailable." : null;
		SetBackingField(model, nameof(WalletViewModel.SyncStatus), sync);
		var view = new WalletView { DataContext = model };
		view.DetachedFromVisualTree += (_, _) => { command.Dispose(); receiveCommand.Dispose(); };
		return view;
	}

	[SuppressMessage("Reliability", "CA2000:Dispose objects before losing scope", Justification = "Commands are disposed when the returned view detaches.")]
	public static Control CreateTransactionPreview(UiContext context, Action? confirm = null, decimal? usdExchangeRate = 100_000m)
	{
		var parent = NewModel<TransactionPreviewViewModel>(context);
		SetField(parent, "_walletModel", NewWallet());
		parent.NextButtonText = "Confirm";
		var confirmCommand = ReactiveCommand.Create(() => confirm?.Invoke());
		SetProperty(parent, nameof(TransactionPreviewViewModel.NextCommand), confirmCommand);
		var destination = ExtKey.CreateFromSeed(new byte[32]).Neuter().PubKey.GetAddress(ScriptPubKeyType.Segwit, Network.RegTest);
		var info = new TransactionInfo(new Destination(destination.ScriptPubKey), 50);
		var services = DispatchProxy.Create<IServices, AutomaticPreviewServices>();
		((AutomaticPreviewServices)(object)services).UsdExchangeRate = usdExchangeRate ?? 0m;
		var amountProvider = new AmountProvider(services);
		var summary = new TransactionSummaryViewModel(context, parent, NewWallet(), info)
		{
			Amount = new Amount(Money.Coins(0.01m)), Fee = amountProvider.Create(Money.Satoshis(280)),
			Recipient = new LabelsArray("synthetic-payment")
		};
		var privacy = NewModel<PrivacySuggestionsFlyoutViewModel>(context);
		privacy.GoodPrivacy = true;
		SetBackingField(parent, nameof(TransactionPreviewViewModel.PrivacySuggestions), privacy);
		SetBackingField(parent, nameof(TransactionPreviewViewModel.CurrentTransactionSummary), summary);
		SetBackingField(parent, nameof(TransactionPreviewViewModel.TransactionSummaries), new List<TransactionSummaryViewModel> { summary });
		parent.DisplayedTransactionSummary = summary;
		var view = new TransactionPreviewView { DataContext = parent };
		view.DetachedFromVisualTree += (_, _) => { confirmCommand.Dispose(); };
		return view;
	}

	[SuppressMessage("Reliability", "CA2000:Dispose objects before losing scope", Justification = "The returned view owns the coin model and its caches.")]
	public static Control CreateWalletCoins(UiContext context)
	{
		var coins = new PreviewCoinList();
		var model = new CoinListViewModel(context, coins);
		model.ExpandAllCommand.Execute().Subscribe();
		var page = NewModel<WalletCoinsViewModel>(context);
		SetProperty(page, nameof(WalletCoinsViewModel.Title), "Wallet Coins");
		SetBackingField(page, nameof(WalletCoinsViewModel.CoinList), model);
		var doneCommand = ReactiveCommand.Create(() => { });
		SetProperty(page, nameof(WalletCoinsViewModel.NextCommand), doneCommand);
		var view = new WalletCoinsView { DataContext = page };
		view.DetachedFromVisualTree += (_, _) => { model.Dispose(); coins.Dispose(); doneCommand.Dispose(); };
		return view;
	}

	public static Control CreateWalletSettings(UiContext context)
	{
		var settings = NewModel<WalletSettingsViewModel>(context);
		var wallet = NewWallet(severalTypes: true);
		SetField(settings, "_wallet", wallet);
		settings.DefaultReceiveScriptType = MagicalCryptoWallet.Fluent.Models.Wallets.ScriptType.SegWit;
		settings.ChangeScriptPubKeyType = PreferredScriptPubKeyType.Unspecified.Instance;
		SetBackingField(settings, nameof(WalletSettingsViewModel.ReceiveScriptTypes), new[] { MagicalCryptoWallet.Fluent.Models.Wallets.ScriptType.SegWit, MagicalCryptoWallet.Fluent.Models.Wallets.ScriptType.Taproot });
		SetBackingField(settings, nameof(WalletSettingsViewModel.ChangeScriptPubKeyTypes), new PreferredScriptPubKeyType[] { PreferredScriptPubKeyType.Unspecified.Instance, PreferredScriptPubKeyType.Specified.SegWit, PreferredScriptPubKeyType.Specified.Taproot });
		var view = new WalletGeneralSettingsView { DataContext = settings, Margin = new Thickness(24) };
		view.DetachedFromVisualTree += (_, _) => (wallet as IDisposable)?.Dispose();
		return view;
	}

	internal static T NewModel<T>(UiContext context) where T : ViewModelBase
	{
		var model = (T)RuntimeHelpers.GetUninitializedObject(typeof(T));
		typeof(ViewModelBase).GetConstructor([typeof(UiContext)])!.Invoke(model, [context]);
		// The application enables pointer input only for its active routed page.
		typeof(T).GetProperty("IsActive")?.SetValue(model, true);
		return model;
	}
	private static IWalletModel NewWallet(bool severalTypes = false)
	{
		var model = DispatchProxy.Create<IWalletModel, AutomaticPreviewWallet>();
		((AutomaticPreviewWallet)(object)model).SeveralTypes = severalTypes;
		return model;
	}
	private static SmartCoin[] NewCoins()
	{
		var keys = KeyManager.CreateNew(new Mnemonic("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"), "", Network.RegTest);
		var transaction = Transaction.Create(Network.RegTest);
		transaction.Inputs.Add(new OutPoint(uint256.One, 0));
		var first = keys.GetKeys()[0];
		var second = keys.GetKeys()[1];
		first.SetLabel("synthetic-income");
		second.SetLabel("synthetic-income");
		first.SetAnonymitySet(1);
		second.SetAnonymitySet(12);
		transaction.Outputs.Add(new TxOut(Money.Coins(0.02m), first.GetAssumedScriptPubKey()));
		transaction.Outputs.Add(new TxOut(Money.Coins(0.035m), second.GetAssumedScriptPubKey()));
		var smart = new SmartTransaction(transaction, new Height.ChainHeight(100));
		return [new SmartCoin(smart, 0, first), new SmartCoin(smart, 1, second)];
	}
	internal static void SetBackingField(object target, string name, object value) => SetField(target, $"<{name}>k__BackingField", value);
	private static void SetField(object target, string name, object value) => target.GetType().GetField(name, BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
	private static void SetProperty(object target, string name, object value) => target.GetType().GetProperty(name)!.SetValue(target, value);
	private static void CheckView(Control view, Action<Window> assertion)
	{
		var panel = new DockPanel();
		var title = new TitleBar { Height = 46 };
		DockPanel.SetDock(title, Dock.Top);
		panel.Children.Add(title);
		panel.Children.Add(view);
		var window = new Window { Width = 900, Height = 650, Content = panel };
		window.Show(); Flush(); window.Measure(new Size(900, 650)); window.Arrange(new Rect(0, 0, 900, 650));
		try { Flush(); assertion(window); }
		catch
		{
			Directory.CreateDirectory(".artifacts/coin-control");
			using var bitmap = new Avalonia.Media.Imaging.RenderTargetBitmap(new PixelSize(900, 650));
			bitmap.Render(window); bitmap.Save($".artifacts/coin-control/debug-{view.GetType().Name}.png");
			throw;
		}
		finally { window.Close(); }
	}
	private static void Click(Window window, Control control)
	{
		var point = control.TranslatePoint(new Point(control.Bounds.Width / 2, control.Bounds.Height / 2), window)!.Value;
		window.MouseMove(point); window.MouseDown(point, MouseButton.Left); window.MouseUp(point, MouseButton.Left); Flush();
	}
	private static void Flush()
	{
		Dispatcher.UIThread.RunJobs();
		AvaloniaHeadlessPlatform.ForceRenderTimerTick();
		Dispatcher.UIThread.RunJobs();
	}
	private static void Check(bool condition, string message) { if (!condition) throw new InvalidOperationException(message); }
	private sealed class PreviewCoinList : ICoinListModel, IDisposable
	{
		private readonly SourceCache<CoinModel, int> _list = new(c => c.Key);
		private readonly SourceCache<Pocket, LabelsArray> _pockets = new(p => p.Labels);
		public PreviewCoinList()
		{
			var coins = NewCoins();
			var services = DispatchProxy.Create<IServices, AutomaticPreviewServices>();
			_list.AddOrUpdate(coins.Select(c => new CoinModel(c, Network.RegTest, 50, services)));
			_pockets.AddOrUpdate(new Pocket((new LabelsArray("synthetic-income"), new CoinsView(coins))));
		}
		public IObservableCache<CoinModel, int> List => _list;
		public IObservableCache<Pocket, LabelsArray> Pockets => _pockets;
		public CoinModel GetCoinModel(SmartCoin coin) => _list.Items.Single(c => c.Key == coin.Outpoint.GetHashCode());
		public void Dispose() { _list.Dispose(); _pockets.Dispose(); }
	}
}

public class AutomaticPreviewWallet : DispatchProxy
{
	public bool SeveralTypes { get; set; }
	private readonly WalletSettingsModel _settings = (WalletSettingsModel)RuntimeHelpers.GetUninitializedObject(typeof(WalletSettingsModel));
	protected override object? Invoke(MethodInfo? method, object?[]? args) => method?.Name switch
	{
		"get_Network" => Network.RegTest,
		"get_Settings" => _settings,
		"get_SeveralReceivingScriptTypes" => SeveralTypes,
		_ => throw new InvalidOperationException("Wallet services are unavailable in this preview.")
	};
}

public class AutomaticPreviewServices : DispatchProxy
{
	private readonly MagicalCryptoWallet.Services.EventBus _events = new();
	public decimal UsdExchangeRate { get; set; }
	protected override object? Invoke(MethodInfo? method, object?[]? args) => method?.Name switch
	{
		"GetServerTipHeight" => 120u,
		"get_EventBus" => _events,
		"GetUsdExchangeRate" => UsdExchangeRate,
		_ => throw new InvalidOperationException("Network services are unavailable in this preview.")
	};
}
