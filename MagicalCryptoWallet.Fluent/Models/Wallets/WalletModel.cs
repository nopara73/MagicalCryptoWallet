using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Collections.Generic;
using System.ComponentModel;
using System.Linq;
using System.Reactive.Linq;
using NBitcoin;
using ReactiveUI;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.Models.Transactions;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Labels;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public partial interface IWalletModel : INotifyPropertyChanged
{



	WalletSessionSnapshot SessionStatus { get; }
	IObservable<WalletSessionSnapshot> Status { get; }

	IObservable<bool> IsCoinjoinRunning { get; }

	IObservable<bool> IsCoinjoinStarted { get; }

	bool IsCoinJoinEnabled { get; }

	AddressesModel Addresses { get; }



	Network Network { get; }

	IEnumerable<ScriptPubKeyType> AvailableScriptPubKeyTypes { get; }

	bool SeveralReceivingScriptTypes { get; }

	WalletTransactionsModel Transactions { get; }

	IObservable<Amount> Balances { get; }

	IObservable<bool> HasBalance { get; }

	WalletCoinsModel Coins { get; }

	WalletAuthorizationModel Auth { get; }

	WalletSyncProgress SyncProgress { get; }

	WalletSettingsModel Settings { get; }

	WalletPrivacyModel Privacy { get; }

	WalletCoinjoinModel? Coinjoin { get; }


	AmountProvider AmountProvider { get; }



	IEnumerable<(string Label, int Score)> GetMostUsedLabels(Intent intent);

	IWalletStatsModel GetWalletStats();

	WalletInfoModel GetWalletInfo();

	PrivacySuggestionsModel GetPrivacySuggestionsModel(SendFlowModel sendFlow);

}

[AppLifetime]
public partial class WalletModel : ReactiveObject, IWalletModel, IDisposable
{
	private readonly CompositeDisposable _lifetime = new();
	private readonly IServices _services;
	private readonly Lazy<WalletCoinjoinModel?> _coinjoin;
	private readonly Lazy<WalletCoinsModel> _coins;

	[AutoNotify] private WalletSessionSnapshot _sessionStatus;

	public WalletModel(IServices services, Wallet wallet, AmountProvider amountProvider)
	{
		_services = services;
		Wallet = wallet;
		AmountProvider = amountProvider;

		Auth = new WalletAuthorizationModel(services.WalletSession, Wallet);
		_sessionStatus = services.WalletSession.Snapshot;
		Status = Observable.Create<WalletSessionSnapshot>(observer => services.WalletSession.Subscribe(observer.OnNext))
			.ObserveOn(RxApp.MainThreadScheduler).Replay(1).RefCount();
		Status.BindTo(this, x => x.SessionStatus).DisposeWith(_lifetime);
		Status.Subscribe(_ =>
		{
			this.RaisePropertyChanged(nameof(AvailableScriptPubKeyTypes));
			this.RaisePropertyChanged(nameof(SeveralReceivingScriptTypes));
		}).DisposeWith(_lifetime);
		SyncProgress = new WalletSyncProgress(services, Wallet);
		Settings = new WalletSettingsModel(services, Wallet.KeyManager);

		_coinjoin = new(() =>
		{
			var coinJoinManager = services.GetHostedService<CoinJoinManager>();
			return coinJoinManager is not null
				? new WalletCoinjoinModel(coinJoinManager)
				: null;
		});

		_coins = new(() => new WalletCoinsModel(wallet, this, services));

		Transactions = new WalletTransactionsModel(services, this, wallet);

		Addresses = new AddressesModel(services, Wallet);

		Privacy = new WalletPrivacyModel(this, Wallet);

		Balances = Transactions.TransactionProcessed.Merge(Status.Where(x => x.HasCachedData).Select(_ => System.Reactive.Unit.Default))
			.Where(_ => services.WalletSession.Snapshot.HasCachedData)
			.Select(_ => Wallet.Coins.TotalAmount())
			.Select(AmountProvider.Create);

		HasBalance = Balances.Select(x => x.HasBalance);


	}

	public IObservable<WalletSessionSnapshot> Status { get; }

	public IObservable<bool> IsCoinjoinRunning => _coinjoin.Value?.IsRunning ?? Observable.Return(false);

	public IObservable<bool> IsCoinjoinStarted => _coinjoin.Value?.IsStarted ?? Observable.Return(false);

	public bool IsCoinJoinEnabled => _coinjoin.Value is not null;

	public AddressesModel Addresses { get; }

	internal Wallet Wallet { get; }


	public Network Network => Wallet.Network;

	public IEnumerable<ScriptPubKeyType> AvailableScriptPubKeyTypes => Wallet.KeyManager.AvailableScriptPubKeyTypes;

	public bool SeveralReceivingScriptTypes => AvailableScriptPubKeyTypes.Contains(ScriptPubKeyType.TaprootBIP86);

	public WalletTransactionsModel Transactions { get; }

	public IObservable<Amount> Balances { get; }

	public IObservable<bool> HasBalance { get; }

	public WalletCoinsModel Coins => _coins.Value;

	public WalletAuthorizationModel Auth { get; }

	public WalletSyncProgress SyncProgress { get; }

	public WalletSettingsModel Settings { get; }

	public WalletPrivacyModel Privacy { get; }

	public WalletCoinjoinModel? Coinjoin => _coinjoin.Value;

	public AmountProvider AmountProvider { get; }



	public IEnumerable<(string Label, int Score)> GetMostUsedLabels(Intent intent)
	{
		return Wallet.GetLabelsWithRanking(intent, _services);
	}

	public IWalletStatsModel GetWalletStats()
	{
		return new WalletStatsModel(this, Wallet);
	}

	public WalletInfoModel GetWalletInfo()
	{
		return new WalletInfoModel(Wallet);
	}

	public PrivacySuggestionsModel GetPrivacySuggestionsModel(SendFlowModel sendFlow)
	{
		return new PrivacySuggestionsModel(_services, sendFlow);
	}

	public void Dispose() { _lifetime.Dispose(); SyncProgress.Dispose(); Transactions.Dispose(); Settings.Dispose(); if (_coinjoin.IsValueCreated) { _coinjoin.Value?.Dispose(); } }

}
