using NBitcoin;
using System.Linq;
using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Hwi.Models;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public partial class WalletRepository : ReactiveObject
{
	private readonly IServices _services;
	private readonly AmountProvider _amountProvider;
	private readonly CompositeDisposable _disposable = new();

	public WalletRepository(IServices services, AmountProvider amountProvider)
	{
		_services = services;
		_amountProvider = amountProvider;

		Observable.FromEventPattern<Wallet>(services.WalletManager, nameof(WalletManager.WalletAdded))
			.Select(x => x.EventArgs)
			.Subscribe(wallet => Wallet = CreateWalletModel(wallet))
			.DisposeWith(_disposable);

		if (services.WalletManager.GetWallet() is { } wallet)
		{
			Wallet = CreateWalletModel(wallet);
		}
	}

	public IWalletModel? Wallet
	{
		get;
		private set => this.RaiseAndSetIfChanged(ref field, value);
	}

	public bool HasWallet => _services.HasWallet();

	private KeyPath AccountKeyPath => KeyManager.GetAccountKeyPath(_services.GetNetwork(), ScriptPubKeyType.Segwit);

	public string GetNextWalletName()
	{
		return _services.GetNextWalletName("Wallet");
	}

	public async Task<WalletSettingsModel> NewWalletAsync(WalletCreationOptions options, CancellationToken? cancelToken = null)
	{
		_services.WalletManager.EnsureCanAddWallet();
		return options switch
		{
			WalletCreationOptions.AddNewWallet add => await CreateNewWalletAsync(add),
			WalletCreationOptions.ConnectToHardwareWallet hw => await ConnectToHardwareWalletAsync(hw, cancelToken),
			WalletCreationOptions.ImportWallet import => await ImportWalletAsync(import),
			WalletCreationOptions.RecoverWallet recover => await RecoverWalletAsync(recover),
			_ => throw new InvalidOperationException($"{nameof(WalletCreationOptions)} not supported: {options?.GetType().Name}")
		};
	}

	public IWalletModel SaveWallet(WalletSettingsModel walletSettings)
	{
		var id = walletSettings.Save();
		var result = Wallet is { } wallet && wallet.Id == id ? wallet : throw new InvalidOperationException("The configured wallet was not found.");
		result.Settings.IsCoinJoinPaused = walletSettings.IsCoinJoinPaused;
		return result;
	}

	public (ErrorSeverity Severity, string Message)? ValidateWalletName(string walletName)
	{
		return _services.ValidateWalletName(walletName);
	}

	public IWalletModel? GetExistingWallet(HwiEnumerateEntry device) =>
		_services.WalletManager.GetWallet() is { } wallet && device.Fingerprint is { } fingerprint && wallet.KeyManager.MasterFingerprint == fingerprint ? Wallet : null;

	private async Task<WalletSettingsModel> CreateNewWalletAsync(WalletCreationOptions.AddNewWallet options)
	{
		var (walletName, walletBackup, _) = options;

		ArgumentException.ThrowIfNullOrEmpty(walletName);
		ArgumentNullException.ThrowIfNull(walletBackup);
		ArgumentNullException.ThrowIfNull(walletBackup.Password);

		var keyManager = await Task.Run(
				() =>
				{
					var walletGenerator = new WalletGenerator(
						_services.GetWalletsDir(),
						_services.GetNetwork())
					{
						TipHeight = _services.GetTipHeight()
					};

					return walletBackup switch
					{
						RecoveryWordsBackup recoveryWordsBackup =>
							walletGenerator.GenerateWallet(
								walletName,
								recoveryWordsBackup.Password,
								recoveryWordsBackup.Mnemonic, toFile: false).KeyManager,
						MultiShareBackup multiShareBackup =>
							walletGenerator.GenerateWallet(
								walletName,
								multiShareBackup.Password,
								multiShareBackup.Shares.Take(multiShareBackup.Settings.Threshold).ToArray(), toFile: false).KeyManager,
						_ => throw new ArgumentOutOfRangeException(nameof(walletBackup))
					};
				});

		return new WalletSettingsModel(_services, keyManager, true);
	}

	private async Task<WalletSettingsModel> ConnectToHardwareWalletAsync(WalletCreationOptions.ConnectToHardwareWallet options, CancellationToken? cancelToken)
	{
		var (walletName, device) = options;

		ArgumentException.ThrowIfNullOrEmpty(walletName);
		ArgumentNullException.ThrowIfNull(device);
		ArgumentNullException.ThrowIfNull(cancelToken);

		var walletFilePath = _services.GetWalletFilePath(walletName);
		var keyManager = await HardwareWalletOperationHelpers.GenerateWalletAsync(device, walletFilePath, _services.GetNetwork(), cancelToken.Value, toFile: false);
		keyManager.SetIcon(device.WalletType, toFile: false);

		var result = new WalletSettingsModel(_services, keyManager, true);
		return result;
	}

	private async Task<WalletSettingsModel> ImportWalletAsync(WalletCreationOptions.ImportWallet options)
	{
		var (walletName, filePath) = options;

		ArgumentException.ThrowIfNullOrEmpty(walletName);
		ArgumentException.ThrowIfNullOrEmpty(filePath);

		var keyManager = await ImportWalletHelper.ImportWalletAsync(_services.WalletManager, walletName, filePath);
		return new WalletSettingsModel(_services, keyManager, true);
	}

	private async Task<WalletSettingsModel> RecoverWalletAsync(WalletCreationOptions.RecoverWallet options)
	{
		var (walletName, walletBackup, minGapLimit, birthHeight) = options;

		ArgumentException.ThrowIfNullOrEmpty(walletName);
		ArgumentNullException.ThrowIfNull(minGapLimit);
		ArgumentNullException.ThrowIfNull(walletBackup);

		var keyManager = await Task.Run(() =>
		{
			var walletFilePath = _services.GetWalletFilePath(walletName);

			var result = walletBackup switch
			{
				RecoveryWordsBackup recoveryWordsBackup =>
					KeyManager.Recover(
						recoveryWordsBackup.Mnemonic,
						recoveryWordsBackup.Password,
						_services.GetNetwork(),
						AccountKeyPath,
						null,
						"", // Make sure it is not saved into a file yet.
						minGapLimit.Value,
						birthHeight: birthHeight),
				MultiShareBackup multiShareBackup =>
					KeyManager.Recover(
						multiShareBackup.Shares,
						multiShareBackup.Password,
						_services.GetNetwork(),
						AccountKeyPath,
						null,
						"", // Make sure it is not saved into a file yet.
						minGapLimit.Value,
						birthHeight: birthHeight),
				_ => throw new ArgumentOutOfRangeException(nameof(walletBackup))
			};

			// Set the filepath but we will only write the file later when the Ui workflow is done.
			result.SetFilePath(walletFilePath);

			return result;
		});

		return new WalletSettingsModel(_services, keyManager, true, true);
	}

	private WalletModel CreateWalletModel(Wallet wallet) =>
		wallet.KeyManager.IsHardwareWallet
		? new HardwareWalletModel(_services, wallet, _amountProvider)
		: new WalletModel(_services, wallet, _amountProvider);
}
