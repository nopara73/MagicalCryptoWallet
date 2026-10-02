using NBitcoin;
using System.Linq;
using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using System.Threading.Tasks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public partial class WalletSetupService : ReactiveObject, IDisposable
{
	private readonly IServices _services;
	private readonly AmountProvider _amountProvider;
	private readonly CompositeDisposable _disposable = new();

	public WalletSetupService(IServices services, AmountProvider amountProvider)
	{
		_services = services;
		_amountProvider = amountProvider;

		Observable.FromEventPattern<Wallet>(services.WalletSession, nameof(WalletSession.WalletConfigured))
			.Select(x => x.EventArgs)
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(PublishWallet)
			.DisposeWith(_disposable);

		if (services.WalletSession.GetWallet() is { } wallet)
		{
			PublishWallet(wallet);
		}
	}

	public IWalletModel? Wallet
	{
		get;
		private set => this.RaiseAndSetIfChanged(ref field, value);
	}

	public bool HasWallet => _services.WalletSession.HasWallet();

	private KeyPath AccountKeyPath => KeyManager.GetAccountKeyPath(_services.GetNetwork(), ScriptPubKeyType.Segwit);

	public async Task<WalletSetupDraft> NewWalletAsync(WalletCreationOptions options)
	{
		_services.WalletSession.EnsureCanConfigure();
		return options switch
		{
			WalletCreationOptions.AddNewWallet add => await CreateNewWalletAsync(add),
			WalletCreationOptions.ImportWallet import => await ImportWalletAsync(import),
			WalletCreationOptions.RecoverWallet recover => await RecoverWalletAsync(recover),
			_ => throw new InvalidOperationException($"{nameof(WalletCreationOptions)} not supported: {options?.GetType().Name}")
		};
	}

	public async Task<IWalletModel> CommitAsync(WalletSetupDraft draft, string? password = null)
	{
		var wallet = await Task.Run(() => _services.WalletSession.Configure(draft.Keys, password));
		PublishWallet(wallet);
		var result = Wallet ?? throw new InvalidOperationException("The configured wallet model is unavailable.");
		return result;
	}

	private void PublishWallet(Wallet wallet)
	{
		if (Wallet is WalletModel model && ReferenceEquals(model.Wallet, wallet)) { return; }
		var old = Wallet;
		Wallet = CreateWalletModel(wallet);
		(old as IDisposable)?.Dispose();
	}

	private async Task<WalletSetupDraft> CreateNewWalletAsync(WalletCreationOptions.AddNewWallet options)
	{
		var (walletBackup, _) = options;

		ArgumentNullException.ThrowIfNull(walletBackup);
		ArgumentNullException.ThrowIfNull(walletBackup.Password);

		var keyManager = await Task.Run(
				() =>
				{
					var walletGenerator = new WalletGenerator(
						_services.WalletSession.WalletDirectories.WalletsDir,
						_services.GetNetwork())
					{
						TipHeight = _services.GetTipHeight()
					};

					return walletBackup switch
					{
						RecoveryWordsBackup recoveryWordsBackup =>
							walletGenerator.GenerateDraft(
								recoveryWordsBackup.Password,
								recoveryWordsBackup.Mnemonic).KeyManager,
						MultiShareBackup multiShareBackup =>
							walletGenerator.GenerateDraft(
								multiShareBackup.Password,
								multiShareBackup.Shares.Take(multiShareBackup.Settings.Threshold).ToArray()).KeyManager,
						_ => throw new ArgumentOutOfRangeException(nameof(walletBackup))
					};
				});

		return new WalletSetupDraft(keyManager);
	}

	private async Task<WalletSetupDraft> ImportWalletAsync(WalletCreationOptions.ImportWallet options)
	{
		var filePath = options.FilePath;

		ArgumentException.ThrowIfNullOrEmpty(filePath);

		var keyManager = await ImportWalletHelper.ImportWalletAsync(_services.WalletSession, filePath);
		return new WalletSetupDraft(keyManager);
	}

	private async Task<WalletSetupDraft> RecoverWalletAsync(WalletCreationOptions.RecoverWallet options)
	{
		var (walletBackup, minGapLimit, birthHeight) = options;

		ArgumentNullException.ThrowIfNull(minGapLimit);
		ArgumentNullException.ThrowIfNull(walletBackup);

		var keyManager = await Task.Run(() =>
		{
			var walletFilePath = _services.WalletSession.WalletDirectories.NewWalletFilePath;

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

		return new WalletSetupDraft(keyManager);
	}

	private WalletModel CreateWalletModel(Wallet wallet) =>
		new WalletModel(_services, wallet, _amountProvider);
	public void Dispose() { _disposable.Dispose(); (Wallet as IDisposable)?.Dispose(); }

}
