using System.Diagnostics.CodeAnalysis;
using System.Reflection;
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
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Transactions.Inputs;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Transactions.Outputs;
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
		int sends = 0, adjustments = 0, confirmations = 0;
		CheckView(CreateWalletActions(context, () => sends++), window =>
		{
			var send = window.GetVisualDescendants().OfType<Button>().Single(b => ControlAutomationPeer.CreatePeerForElement(b).GetName() == "Send");
			Check(send.Flyout is null && !window.GetVisualDescendants().OfType<SubActionButton>().Any(b => ReferenceEquals(b.Command, send.Command)), "Send must be one action without an input-selection menu.");
			Click(window, send);
			Check(sends == 1, "The real Send button must activate exactly once.");
		});
		CheckView(CreateTransactionPreview(context, () => adjustments++, () => confirmations++), window =>
		{
			window.KeyPress(Key.LeftAlt, RawInputModifiers.Alt, PhysicalKey.AltLeft, "");
			Flush();
			Check(!window.GetVisualDescendants().OfType<Button>().Any(b => Equals(b.Content, "Review coins")), "Alt must not reveal input selection.");
			window.KeyRelease(Key.LeftAlt, RawInputModifiers.None, PhysicalKey.AltLeft, "");
			var fee = window.GetVisualDescendants().OfType<Button>().Single(b => Equals(ToolTip.GetTip(b), "Change transaction fee or confirmation time"));
			Check(fee.IsEnabled && fee.IsVisible, "Fee adjustment must remain available.");
			Click(window, fee);
			var confirm = window.GetVisualDescendants().OfType<Button>().Single(b => Equals(b.Content, "Confirm"));
			Click(window, confirm);
			Check(adjustments == 1 && confirmations == 1, "Fee and confirmation commands must remain operable.");
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
		Console.WriteLine("Automatic coin selection UI checks passed: one accessible Send action, no Alt input selection, working fee/confirm buttons, read-only sortable coin details, and no manual send setting.");
	}

	[SuppressMessage("Reliability", "CA2000:Dispose objects before losing scope", Justification = "Commands are disposed when the returned view detaches.")]
	public static Control CreateWalletActions(UiContext context, Action? send = null)
	{
		var model = NewModel<WalletViewModel>(context);
		SetProperty(model, nameof(WalletViewModel.Title), "My Wallet");
		SetProperty(model, nameof(WalletViewModel.IsSendButtonVisible), true);
		SetBackingField(model, nameof(WalletViewModel.WalletModel), NewWallet());
		SetBackingField(model, nameof(WalletViewModel.Settings), NewModel<WalletSettingsViewModel>(context));
		SetBackingField(model, nameof(WalletViewModel.Tiles), Array.Empty<ActivatableViewModel>());
		var command = ReactiveCommand.Create(() => send?.Invoke());
		var receiveCommand = ReactiveCommand.Create(() => { });
		SetProperty(model, nameof(WalletViewModel.SendCommand), command);
		SetProperty(model, nameof(WalletViewModel.SegwitReceiveCommand), receiveCommand);
		model.DefaultReceiveCommand = receiveCommand;
		var view = new WalletView { DataContext = model };
		view.DetachedFromVisualTree += (_, _) => { command.Dispose(); receiveCommand.Dispose(); };
		return view;
	}

	[SuppressMessage("Reliability", "CA2000:Dispose objects before losing scope", Justification = "Commands and lists are disposed when the returned view detaches.")]
	public static Control CreateTransactionPreview(UiContext context, Action? adjust = null, Action? confirm = null)
	{
		var parent = NewModel<TransactionPreviewViewModel>(context);
		SetField(parent, "_walletModel", NewWallet());
		parent.NextButtonText = "Confirm";
		parent.IsFeeAdjustable = true;
		parent.FeeAdjustToolTip = "Change transaction fee or confirmation time";
		var feeCommand = ReactiveCommand.Create(() => adjust?.Invoke());
		var confirmCommand = ReactiveCommand.Create(() => confirm?.Invoke());
		SetBackingField(parent, nameof(TransactionPreviewViewModel.AdjustFeeCommand), feeCommand);
		SetProperty(parent, nameof(TransactionPreviewViewModel.NextCommand), confirmCommand);
		var destination = ExtKey.CreateFromSeed(new byte[32]).Neuter().PubKey.GetAddress(ScriptPubKeyType.Segwit, Network.RegTest);
		var info = new TransactionInfo(new Destination.Loudly(destination.ScriptPubKey), 50);
		var summary = new TransactionSummaryViewModel(context, parent, NewWallet(), info)
		{
			Amount = new Amount(Money.Coins(0.01m)), Fee = new Amount(Money.Satoshis(280)), FeeRate = new FeeRate(2m),
			ConfirmationTime = TimeSpan.FromMinutes(20), Recipient = new LabelsArray("synthetic-payment"),
			InputList = new InputsCoinListViewModel(context, NewCoins(), Network.RegTest, 2),
			OutputList = new OutputsCoinListViewModel(context,
				[new TxOut(Money.Coins(0.0449972m), NewCoins()[0].ScriptPubKey)],
				[new TxOut(Money.Coins(0.01m), destination.ScriptPubKey)], Network.RegTest, new HashSet<Script> { destination.ScriptPubKey })
		};
		var privacy = NewModel<PrivacySuggestionsFlyoutViewModel>(context);
		privacy.GoodPrivacy = true;
		SetBackingField(parent, nameof(TransactionPreviewViewModel.PrivacySuggestions), privacy);
		SetBackingField(parent, nameof(TransactionPreviewViewModel.CurrentTransactionSummary), summary);
		SetBackingField(parent, nameof(TransactionPreviewViewModel.TransactionSummaries), new List<TransactionSummaryViewModel> { summary });
		parent.DisplayedTransactionSummary = summary;
		var view = new TransactionPreviewView { DataContext = parent };
		view.DetachedFromVisualTree += (_, _) => { feeCommand.Dispose(); confirmCommand.Dispose(); summary.InputList.Dispose(); summary.OutputList.Dispose(); };
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
		SetField(settings, "_wallet", NewWallet(severalTypes: true));
		settings.WalletName = "My Wallet";
		settings.DefaultReceiveScriptType = MagicalCryptoWallet.Fluent.Models.Wallets.ScriptType.SegWit;
		settings.ChangeScriptPubKeyType = PreferredScriptPubKeyType.Unspecified.Instance;
		SetBackingField(settings, nameof(WalletSettingsViewModel.ReceiveScriptTypes), new[] { MagicalCryptoWallet.Fluent.Models.Wallets.ScriptType.SegWit, MagicalCryptoWallet.Fluent.Models.Wallets.ScriptType.Taproot });
		SetBackingField(settings, nameof(WalletSettingsViewModel.ChangeScriptPubKeyTypes), new PreferredScriptPubKeyType[] { PreferredScriptPubKeyType.Unspecified.Instance, PreferredScriptPubKeyType.Specified.SegWit, PreferredScriptPubKeyType.Specified.Taproot });
		return new WalletGeneralSettingsView { DataContext = settings, Margin = new Thickness(24) };
	}

	private static T NewModel<T>(UiContext context) where T : ViewModelBase
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
		var key = ExtKey.CreateFromSeed(new byte[32]).Neuter();
		var keys = KeyManager.CreateNewHardwareWalletWatchOnly(key.PubKey.GetHDFingerPrint(), key, null, null, null, Network.RegTest);
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
	private static void SetBackingField(object target, string name, object value) => SetField(target, $"<{name}>k__BackingField", value);
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
		"get_IsWatchOnlyWallet" => false,
		_ => throw new InvalidOperationException("Wallet services are unavailable in this preview.")
	};
}

public class AutomaticPreviewServices : DispatchProxy
{
	protected override object? Invoke(MethodInfo? method, object?[]? args) => method?.Name == "GetServerTipHeight"
		? 120u : throw new InvalidOperationException("Network services are unavailable in this preview.");
}
