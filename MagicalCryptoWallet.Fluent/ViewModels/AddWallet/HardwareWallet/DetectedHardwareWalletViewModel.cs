using System.Reactive.Disposables;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Input;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.AddWallet.HardwareWallet;

[NavigationMetaData(Title = "Hardware Wallet")]
public partial class DetectedHardwareWalletViewModel : RoutableViewModel
{
	public DetectedHardwareWalletViewModel(UiContext uiContext, WalletCreationOptions.ConnectToHardwareWallet options) : base(uiContext)
	{
		var device = options.Device;

		ArgumentNullException.ThrowIfNull(device);


		Type = device.WalletType;

		TypeName = device.Model.FriendlyName();

		SetupCancel(enableCancel: false, enableCancelOnEscape: false, enableCancelOnPressed: false);

		EnableBack = false;

		NextCommand = ReactiveCommand.CreateFromTask(async () => await OnNextAsync(options));

		NoCommand = ReactiveCommand.Create(OnNo);

		EnableAutoBusyOn(NextCommand);
	}

	public CancellationTokenSource? CancelCts { get; private set; }


	public WalletType Type { get; }

	public string TypeName { get; }

	public ICommand NoCommand { get; }

	private async Task OnNextAsync(WalletCreationOptions.ConnectToHardwareWallet options)
	{
		try
		{
			CancelCts ??= new CancellationTokenSource();
			var walletSettings = await UiContext.WalletSetupService.NewWalletAsync(options, CancelCts.Token);
			Navigate().To().AddedWalletPage(walletSettings, options);
		}
		catch (Exception ex)
		{
			Logger.LogError(ex);
			await ShowErrorAsync(Title, ex.ToUserFriendlyString(), "Error occurred during adding your wallet.");
			Navigate().Back();
		}
	}

	private void OnNo()
	{
		Navigate().Back();
	}

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		base.OnNavigatedTo(isInHistory, disposables);

		var enableCancel = UiContext.WalletSetupService.HasWallet;
		SetupCancel(enableCancel: false, enableCancelOnEscape: enableCancel, enableCancelOnPressed: false);

		disposables.Add(Disposable.Create(() =>
		{
			CancelCts?.Cancel();
			CancelCts?.Dispose();
			CancelCts = null;
		}));
	}
}
