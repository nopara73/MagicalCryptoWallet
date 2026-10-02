using System.Reactive.Disposables.Fluent;
using System.Collections.Generic;
using System.Reactive.Disposables;
using System.Reactive.Linq;
using System.Windows.Input;
using NBitcoin;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.Validation;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using ScriptType = MagicalCryptoWallet.Fluent.Models.Wallets.ScriptType;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Settings;

[AppLifetime]
[NavigationMetaData(
    Title = "Wallet Settings",
    Caption = "Display wallet settings",
    IconName = "nav_wallet_24_regular",
    Order = 2,
    Category = "Wallet",
    Keywords = new[] { "Wallet", "Settings", },
    NavBarPosition = NavBarPosition.None,
    NavigationTarget = NavigationTarget.DialogScreen,
    Searchable = false)]
public partial class WalletSettingsViewModel : RoutableViewModel, IDisposable
{
	private readonly CompositeDisposable _lifetime = new();
    private readonly IWalletModel _wallet;
    [AutoNotify] private bool _preferPsbtWorkflow;
    [AutoNotify] private int _selectedTab;
    [AutoNotify] private ScriptType _defaultReceiveScriptType;
    [AutoNotify] private bool _isSegWitDefaultReceiveScriptType;
    [AutoNotify] private PreferredScriptPubKeyType _changeScriptPubKeyType;

    public WalletSettingsViewModel(UiContext uiContext, IWalletModel walletModel) : base(uiContext)
    {
        _wallet = walletModel;
        walletModel.Status.Subscribe(_ => this.RaisePropertyChanged(nameof(SeveralReceivingScriptTypes))).DisposeWith(_lifetime);
        _preferPsbtWorkflow = walletModel.Settings.PreferPsbtWorkflow;
        _selectedTab = 0;
        IsHardwareWallet = walletModel.IsHardwareWallet;
        IsWatchOnly = walletModel.IsWatchOnlyWallet;

        SetupCancel(enableCancel: true, enableCancelOnEscape: true, enableCancelOnPressed: true);
        NextCommand = ReactiveCommand.Create(() => { _wallet.Settings.Save(); Navigate().Back(); });

        _defaultReceiveScriptType = walletModel.Settings.DefaultReceiveScriptType;
        this.WhenAnyValue(x => x.DefaultReceiveScriptType)
            .Subscribe(value => IsSegWitDefaultReceiveScriptType = value == ScriptType.SegWit).DisposeWith(_lifetime);

        _changeScriptPubKeyType = walletModel.Settings.ChangeScriptPubKeyType switch
        {
            PreferredScriptPubKeyType.Specified s => s.ScriptType switch
            {
                ScriptPubKeyType.TaprootBIP86 => PreferredScriptPubKeyType.Specified.Taproot,
                ScriptPubKeyType.Segwit => PreferredScriptPubKeyType.Specified.SegWit,
                _ => throw new ArgumentOutOfRangeException()
            },
            _ => walletModel.Settings.ChangeScriptPubKeyType
        };

        WalletCoinJoinSettings = new WalletCoinJoinSettingsViewModel(UiContext, walletModel);

        VerifyRecoveryWordsCommand = ReactiveCommand.Create(() => Navigate().To().WalletVerifyRecoveryWords(walletModel));

        ResyncWalletCommand = ReactiveCommand.CreateFromTask(async () =>
        {
            var result = await UiContext.Navigate().To().ResyncWallet(walletModel.GetWalletStats().BirthHeight, walletModel.Settings.MinGapLimit).GetResultAsync();
            if (result is not null)
            {
                walletModel.Settings.RescanWallet(result.StartingHeight, result.MinGapLimit);
                UiContext.Navigate(MetaData.NavigationTarget).Clear();
                AppLifetimeHelper.Shutdown(withShutdownPrevention: true, restart: true);
            }
        });

        this.WhenAnyValue(x => x.DefaultReceiveScriptType)
            .Skip(1)
            .Subscribe(value =>
            {
                walletModel.Settings.DefaultReceiveScriptType = value;
                walletModel.Settings.Save();
            }).DisposeWith(_lifetime);

        this.WhenAnyValue(x => x.ChangeScriptPubKeyType)
            .Skip(1)
            .Subscribe(value =>
            {
                walletModel.Settings.ChangeScriptPubKeyType = value;
                walletModel.Settings.Save();
            }).DisposeWith(_lifetime);

        this.WhenAnyValue(x => x.PreferPsbtWorkflow)
            .Skip(1)
            .Subscribe(value =>
            {
                walletModel.Settings.PreferPsbtWorkflow = value;
                walletModel.Settings.Save();
            }).DisposeWith(_lifetime);
    }

    public bool IsHardwareWallet { get; }
    public bool IsWatchOnly { get; }
    public bool SeveralReceivingScriptTypes => _wallet.SeveralReceivingScriptTypes;

    public IEnumerable<ScriptType> ReceiveScriptTypes { get; } = [ScriptType.SegWit, ScriptType.Taproot];
    public IEnumerable<PreferredScriptPubKeyType> ChangeScriptPubKeyTypes { get; } =
    [
        PreferredScriptPubKeyType.Unspecified.Instance,
        PreferredScriptPubKeyType.Specified.SegWit,
        PreferredScriptPubKeyType.Specified.Taproot
    ];


    public WalletCoinJoinSettingsViewModel WalletCoinJoinSettings { get; private set; }
    public ICommand VerifyRecoveryWordsCommand { get; }
    public ICommand ResyncWalletCommand { get; }

    protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
    {
        base.OnNavigatedTo(isInHistory, disposables);


    }
	public void Dispose() { _lifetime.Dispose(); WalletCoinJoinSettings.Dispose(); }

}
