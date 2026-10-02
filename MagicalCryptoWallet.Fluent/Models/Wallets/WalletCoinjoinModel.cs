using System.Reactive.Disposables.Fluent;
using System.Reactive.Disposables;
using System.Linq;
using System.Reactive.Linq;
using System.Threading;
using System.Threading.Tasks;
using ReactiveUI;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoinProgressEvents;
using MagicalCryptoWallet.WabiSabi.Client.StatusChangedEvents;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

[AppLifetime]
public partial class WalletCoinjoinModel : ReactiveObject, IDisposable
{
	private readonly CompositeDisposable _lifetime = new();
	private readonly CoinJoinManager _coinJoinManager;
	[AutoNotify] private bool _isCoinjoining;

	public WalletCoinjoinModel(CoinJoinManager coinjoinManager)
	{
		_coinJoinManager = coinjoinManager;

		StatusUpdated = Observable.Create<StatusChangedEventArgs>(observer => coinjoinManager.SubscribeStatus(observer.OnNext)).ObserveOn(RxApp.MainThreadScheduler);
		Snapshots = Observable.Create<CoinJoinSnapshot>(observer => coinjoinManager.Subscribe(observer.OnNext)).ObserveOn(RxApp.MainThreadScheduler);
		IsRunning = Snapshots.Select(snapshot => snapshot.IsRunning).DistinctUntilChanged();
		IsStarted = Snapshots.Select(snapshot => snapshot.State != CoinJoinClientState.Idle).DistinctUntilChanged();
		IsRunning.BindTo(this, x => x.IsCoinjoining).DisposeWith(_lifetime);
	}

	public IObservable<StatusChangedEventArgs> StatusUpdated { get; }
	public IObservable<CoinJoinSnapshot> Snapshots { get; }

	public IObservable<bool> IsRunning { get; }

	public IObservable<bool> IsStarted { get; }

	public Task StartAsync(bool overridePlebStop)
	{
		_coinJoinManager.RequestCoinJoinStart(overridePlebStop);
		return Task.CompletedTask;
	}

	public Task StopAsync()
	{
		_coinJoinManager.RequestCoinJoinStop();
		return Task.CompletedTask;
	}
	public void Dispose() { _lifetime.Dispose();  }

}
