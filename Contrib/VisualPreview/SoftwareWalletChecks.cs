using System.Collections.ObjectModel;
using System.Diagnostics.CodeAnalysis;
using System.Reflection;
using System.Runtime.CompilerServices;
using System.Reactive.Linq;
using Avalonia;
using Avalonia.Automation.Peers;
using Avalonia.Controls;
using Avalonia.Headless;
using Avalonia.Input;
using Avalonia.Threading;
using Avalonia.VisualTree;
using NBitcoin;
using ReactiveUI;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Controls;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Models;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels;
using MagicalCryptoWallet.Fluent.ViewModels.AddWallet;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Labels;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Receive;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Send;
using MagicalCryptoWallet.Fluent.Views.AddWallet;
using MagicalCryptoWallet.Fluent.Views.Wallets;
using MagicalCryptoWallet.Fluent.Views.Wallets.Receive;
using MagicalCryptoWallet.Fluent.Views.Wallets.Send;
using MagicalCryptoWallet.Wallets;
using Key = Avalonia.Input.Key;
using ScriptType = MagicalCryptoWallet.Fluent.Models.Wallets.ScriptType;

internal static class SoftwareWalletChecks
{
	public static void Run(UiContext context, string destination)
	{
		Set(context, nameof(UiContext.QrCodeGenerator), new QrCodeGenerator());
		Set(context, nameof(UiContext.QrCodeReader), new QrCodeReader());
		var settings = (ApplicationSettings)RuntimeHelpers.GetUninitializedObject(typeof(ApplicationSettings));
		settings.Network = Network.RegTest;
		Set(context, nameof(UiContext.ApplicationSettings), settings);
		var directories = new WalletDirectories(Network.RegTest, Path.Combine(Path.GetFullPath(destination), "synthetic-recovery"));
		var session = new WalletSession(Network.RegTest, directories, _ => throw new InvalidOperationException("Preview cannot initialize a wallet."));
		Set(Services.Instance, nameof(Services.WalletSession), session);
		session.ReportInitializationFailure(new NotSupportedException("This wallet has no local private keys. Hardware and watch-only wallets are no longer supported. Open it with a compatible wallet application."));
		var services = DispatchProxy.Create<IServices, SoftwarePreviewServices>();
		((SoftwarePreviewServices)(object)services).Session = session;
		Set(context, nameof(UiContext.Services), services);

		int[] activations = new int[3];
		var model = new AddWalletPageViewModel(context);
		model.IsActive = true;
		var commands = Enumerable.Range(0, 3).Select(i => ReactiveCommand.Create(() => activations[i]++)).ToArray();
		Set(model, nameof(AddWalletPageViewModel.CreateWalletCommand), commands[0]);
		Set(model, nameof(AddWalletPageViewModel.ImportWalletCommand), commands[1]);
		Set(model, nameof(AddWalletPageViewModel.RecoverWalletCommand), commands[2]);
		var view = new AddWalletPageView { DataContext = model };
		var window = new Window { Width = 800, Height = 600, Content = view };
		window.Show();
		Dispatcher.UIThread.RunJobs();
		window.Measure(new Size(800, 600));
		window.Arrange(new Rect(0, 0, 800, 600));
		try
		{
			var tiles = view.GetVisualDescendants().OfType<TileButton>().OrderBy(t => Grid.GetRow(t)).ThenBy(t => Grid.GetColumn(t)).ToArray();
			Check(tiles.Length == 3, "Setup must offer exactly Create, Import and Recover.");
			Check(Grid.GetColumnSpan(tiles[0]) == 2 && tiles[0].Bounds.Width > tiles[1].Bounds.Width
				&& Math.Abs(tiles[1].Bounds.Width - tiles[2].Bounds.Width) < 1, "Create must span the equal Import and Recover columns.");
			for (int i = 0; i < tiles.Length; i++)
			{
				Check(ControlAutomationPeer.CreatePeerForElement(tiles[i]).GetName() == tiles[i].Text, "Setup actions need accessible names.");
				Check(tiles[i].Focus(), $"Setup tile {i} must accept keyboard focus.");
				foreach (var (key, physical) in new[] { (Key.Enter, PhysicalKey.Enter), (Key.Space, PhysicalKey.Space) })
				{
					tiles[i].Focus();
					window.KeyPress(key, RawInputModifiers.None, physical, "");
					window.KeyRelease(key, RawInputModifiers.None, physical, "");
					Dispatcher.UIThread.RunJobs();
				}
				Check(activations[i] == 2, $"Setup tile {i} must respond once to Enter and Space; activations={string.Join(',', activations)}, focused={tiles[i].IsFocused}, enabled={tiles[i].IsEnabled}.");
			}
		}
		finally { window.Close(); foreach (var command in commands) command.Dispose(); }
		Console.WriteLine("Software-wallet UI checks passed: exactly three accessible setup actions, full-width Create, equal Import/Recover, actual Enter/Space activation.");
	}

	public static Control CreateReceive(UiContext context)
	{
		var wallet = DispatchProxy.Create<IWalletModel, SoftwarePreviewWallet>();
		var model = new ReceiveAddressViewModel(context, wallet, new PreviewAddress(), false);
		// Generation runs on a worker; finish before taking the first screenshot.
		_ = model.QrCode.FirstAsync().Wait();
		return new ReceiveAddressView { DataContext = model };
	}
	public static Control CreateRecovery(UiContext context) => new WalletRecoveryView { DataContext = new WalletRecoveryViewModel(context) };
	public static Control CreateRecoverWords(UiContext context)
	{
		var model = new RecoverWalletViewModel(context, new WalletCreationOptions.RecoverWallet());
		foreach (var word in "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about".Split(' ')) model.Mnemonics.Add(word);
		return new RecoverWalletView { DataContext = model };
	}
	[SuppressMessage("Reliability", "CA2000:Dispose objects before losing scope", Justification = "The returned view disposes its inert command on detachment.")]
	public static Control CreateSend(UiContext context)
	{
		var model = (SendViewModel)RuntimeHelpers.GetUninitializedObject(typeof(SendViewModel));
		typeof(ViewModelBase).GetConstructor([typeof(UiContext)])!.Invoke(model, [context]);
		model.IsActive = true;
		model.To = ExtKey.CreateFromSeed(new byte[32]).Neuter().PubKey.GetAddress(ScriptPubKeyType.Segwit, Network.RegTest).ToString();
		model.AmountBtc = 0.001m;
		model.BitcoinContent = "0.001 BTC";
		Set(model, nameof(SendViewModel.Balance), Observable.Return(new Amount(Money.Coins(0.055m))));
		Set(model, nameof(SendViewModel.AdditionalRecipients), new IndexedCollection<RecipientRowViewModel>(new ObservableCollection<RecipientRowViewModel>()));
		var wallet = DispatchProxy.Create<IWalletModel, SoftwarePreviewWallet>();
		typeof(SendViewModel).GetProperty(nameof(SendViewModel.SuggestionLabels))!.SetValue(model, new SuggestionLabelsViewModel(context, wallet, Intent.Send, 3, ["synthetic-payment"]));
		var command = ReactiveCommand.Create(() => { });
		Set(model, nameof(SendViewModel.AutoPasteCommand), command);
		typeof(SendViewModel).GetProperty(nameof(SendViewModel.NextCommand))!.SetValue(model, command);
		var view = new SendView { DataContext = model };
		view.DetachedFromVisualTree += (_, _) => command.Dispose();
		return view;
	}
	private static void Set(object target, string name, object value) => target.GetType().GetField($"<{name}>k__BackingField", BindingFlags.Instance | BindingFlags.NonPublic)!.SetValue(target, value);
	private static void Check(bool success, string message) { if (!success) throw new InvalidOperationException(message); }
	private sealed class PreviewAddress : ReactiveObject, IAddress
	{
		public string Text { get; } = ExtKey.CreateFromSeed(new byte[32]).Neuter().PubKey.GetAddress(ScriptPubKeyType.Segwit, Network.RegTest).ToString();
		public string ShortenedText => Text;
		public LabelsArray Labels => new("synthetic-receive");
		public ScriptType ScriptType => ScriptType.SegWit;
		public void Hide() => throw new InvalidOperationException("Preview cannot change an address.");
		public void SetLabels(LabelsArray labels) => throw new InvalidOperationException("Preview cannot change an address.");
	}
}

public class SoftwarePreviewServices : DispatchProxy
{
	public WalletSession Session { get; set; } = null!;
	protected override object? Invoke(MethodInfo? method, object?[]? args) => method?.Name switch
	{
		"get_WalletSession" => Session,
		_ => throw new InvalidOperationException("Preview services are inert.")
	};
}
public class SoftwarePreviewWallet : DispatchProxy
{
	protected override object? Invoke(MethodInfo? method, object?[]? args) => method?.Name switch
	{
		"GetMostUsedLabels" => Array.Empty<(string Label, int Score)>(),
		"Dispose" => null,
		_ => throw new InvalidOperationException("Preview wallet cannot perform operations.")
	};
}
