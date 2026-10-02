using MagicalCryptoWallet.Fluent.Helpers;
using System.Reactive.Disposables;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Advanced;

[NavigationMetaData(
	Title = "Wallet Info",
	Caption = "Display wallet info",
	IconName = "nav_wallet_24_regular",
	Order = 4,
	Category = "Wallet",
	Keywords = new[] { "Wallet", "Info", },
	NavBarPosition = NavBarPosition.None,
	NavigationTarget = NavigationTarget.DialogScreen,
	Searchable = false)]
public partial class WalletInfoViewModel : RoutableViewModel
{
	private readonly WalletInfoModel _model;

	[AutoNotify] private bool _showSensitiveData;
	[AutoNotify] private string _showButtonText = "Show sensitive data";
	[AutoNotify] private string _lockIconString = "eye_show_regular";

	public WalletInfoViewModel(UiContext uiContext, IWalletModel wallet) : base(uiContext)
	{
		_model = wallet.GetWalletInfo();

		SetupCancel(enableCancel: true, enableCancelOnEscape: true, enableCancelOnPressed: true);


		NextCommand = ReactiveCommand.Create(() => Navigate().Clear());

		CancelCommand = ReactiveCommand.CreateFromTask(async () =>
		{
			if (!ShowSensitiveData)
			{
				using var authorization = await AuthorizationHelpers.AuthorizeAsync(UiContext, wallet);
				if (authorization is null) { return; }
				_model.Reveal(authorization);
			}
			else { _model.Dispose(); }
			ShowSensitiveData = !ShowSensitiveData;
			this.RaisePropertyChanged(nameof(ExtendedMasterPrivateKey));
			this.RaisePropertyChanged(nameof(ExtendedAccountPrivateKey));
			this.RaisePropertyChanged(nameof(ExtendedMasterZprv));
			ShowButtonText = ShowSensitiveData ? "Hide sensitive data" : "Show sensitive data";
			LockIconString = ShowSensitiveData ? "eye_hide_regular" : "eye_show_regular";
		});
	}

	public string SegWitExtendedAccountPublicKey => _model.SegWitExtendedAccountPublicKey;

	public string? TaprootExtendedAccountPublicKey => _model.TaprootExtendedAccountPublicKey;

	public string SegWitAccountKeyPath => _model.SegWitAccountKeyPath;

	public string TaprootAccountKeyPath => _model.TaprootAccountKeyPath;

	public string? MasterKeyFingerprint => _model.MasterKeyFingerprint;

	public string? ExtendedMasterPrivateKey => _model.ExtendedMasterPrivateKey;

	public string? ExtendedAccountPrivateKey => _model.ExtendedAccountPrivateKey;

	public string? ExtendedMasterZprv => _model.ExtendedMasterZprv;

	public bool HasWpkhWalletPolicy => _model.WpkhWalletPolicy is not null;

	public string? WpkhWalletPolicyFullDescriptor => _model.WpkhWalletPolicy?.FullDescriptor.ToString();

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		base.OnNavigatedTo(isInHistory, disposables);
		disposables.Add(Disposable.Create(() => { _model.Dispose(); ShowSensitiveData = false; }));
	}
}
