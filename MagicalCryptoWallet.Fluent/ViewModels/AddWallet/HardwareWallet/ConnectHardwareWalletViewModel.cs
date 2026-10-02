using System.Reactive.Disposables;
using System.Reactive.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Input;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Hwi.Models;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Nito.AsyncEx;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.AddWallet.HardwareWallet;

[NavigationMetaData(Title = "Hardware Wallet")]
public partial class ConnectHardwareWalletViewModel : RoutableViewModel
{
	private readonly WalletCreationOptions.ConnectToHardwareWallet _options;
	[AutoNotify] private string _message;
	[AutoNotify] private bool _isSearching;
	[AutoNotify] private bool _confirmationRequired;

	public ConnectHardwareWalletViewModel(UiContext uiContext, WalletCreationOptions.ConnectToHardwareWallet options) : base(uiContext)
	{
		_options = options;

		_message = "";
		AbandonedTasks = new AbandonedTasks();
		CancelCts = new CancellationTokenSource();

		EnableBack = true;

		NextCommand = ReactiveCommand.Create(OnNext);


		this.WhenAnyValue(x => x.Message)
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(message => ConfirmationRequired = !string.IsNullOrEmpty(message));
	}

	private HwiEnumerateEntry? DetectedDevice { get; set; }

	public CancellationTokenSource CancelCts { get; set; }

	private AbandonedTasks AbandonedTasks { get; }




	public WalletType Ledger => WalletType.Ledger;

	public WalletType Coldcard => WalletType.Coldcard;

	public WalletType Trezor => WalletType.Trezor;

	public WalletType Generic => WalletType.Hardware;

	private void OnNext()
	{
		if (DetectedDevice is { } device)
		{
			NavigateToNext(device);
			return;
		}

		StartDetection();
	}

	private void StartDetection()
	{
		Message = "";

		if (IsSearching)
		{
			return;
		}

		DetectedDevice = null;
		AbandonedTasks.AddAndClearCompleted(DetectionAsync(CancelCts.Token));
	}

	private async Task DetectionAsync(CancellationToken cancel)
	{
		IsSearching = true;

		try
		{
			var result = await UiContext.HardwareWalletInterface.DetectAsync(cancel);
			EvaluateDetectionResult(result, cancel);
		}
		catch (Exception ex) when (ex is not OperationCanceledException)
		{
			Logger.LogError(ex);
		}
		finally
		{
			IsSearching = false;
		}
	}

	private void EvaluateDetectionResult(HwiEnumerateEntry[] devices, CancellationToken cancel)
	{
		if (devices.Length == 0)
		{
			Message = "Connect the hardware wallet to the PC / Enter the PIN on the device.";
			return;
		}

		if (devices.Length > 1)
		{
			Message = "Make sure you have only one hardware wallet connected to the PC.";
			return;
		}

		var device = devices[0];

		if (!device.IsInitialized())
		{
			if (device.Model == HardwareWalletModels.Coldcard)
			{
				Message = "Initialize your device first.";
			}
			else
			{
				Message = "Check your device and finish the initialization.";
				AbandonedTasks.AddAndClearCompleted(UiContext.HardwareWalletInterface.InitHardwareWalletAsync(device, cancel));
			}

			return;
		}

		if (device.Code is { })
		{
			Message = "Something happened with your device, unlock it with your PIN/Passphrase or reconnect to the PC.";
			return;
		}

		if (device.NeedsPassphraseSent == true)
		{
			Message = "Enter your passphrase on your device.";
			return;
		}

		if (device.NeedsPinSent == true)
		{
			Message = "Enter your PIN on your device.";
			return;
		}

		DetectedDevice = device;

		if (!ConfirmationRequired)
		{
			NavigateToNext(DetectedDevice);
		}
	}

	private void NavigateToNext(HwiEnumerateEntry device)
	{
		Navigate().To().DetectedHardwareWallet(_options with { Device = device });
	}

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		base.OnNavigatedTo(isInHistory, disposables);

		var enableCancel = UiContext.WalletSetupService.HasWallet;

		SetupCancel(enableCancel: enableCancel, enableCancelOnEscape: enableCancel, enableCancelOnPressed: false);

		if (isInHistory)
		{
			CancelCts = new CancellationTokenSource();
		}

		StartDetection();

		disposables.Add(Disposable.Create(async () =>
		{
			CancelCts.Cancel();
			await AbandonedTasks.WhenAllAsync();
			CancelCts.Dispose();
		}));
	}
}
