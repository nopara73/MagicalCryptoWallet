using System.Collections.Generic;
using System.Linq;
using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Tor.StatusChecker;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public partial class HealthMonitor : ReactiveObject
{
	private readonly ObservableAsPropertyHelper<ICollection<Issue>> _torIssues;

	[AutoNotify] private uint _blockchainTip;
	[AutoNotify] private uint _clientTip;
	[AutoNotify] private TorStatus _torStatus;
	[AutoNotify] private int _peers;
	[AutoNotify] private bool _isP2pConnected;
	[AutoNotify] private HealthMonitorState _state;
	[AutoNotify] private bool _updateAvailable;
	[AutoNotify] private bool _isReadyToInstall;
	[AutoNotify] private bool _checkForUpdates = true;
	[AutoNotify] private Version? _clientVersion;


	public HealthMonitor(IServices services, TorStatusCheckerModel torStatusChecker)
	{
		// Do not make it dynamic, because if you change this config settings only next time will it activate.
		UseTor = services.GetUseTor();
		TorStatus = UseTor == TorMode.Disabled ? TorStatus.TurnedOff : TorStatus.NotRunning;


		// Blockchain Tip
		services.EventBus.AsObservable<NetworkTipHeightChanged>()
			.Select(value => value.Height)
			.WhereNotNull()
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(blockchainTip => BlockchainTip = blockchainTip);

		// Local Tip
		services.EventBus.AsObservable<ClientTipHeightChanged>()
			.Select(value => value.Height)
			.WhereNotNull()
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(clientTip => ClientTip = clientTip);

		// Tor Status
		services.EventBus.AsObservable<TorConnectionStateChanged>()
			.ObserveOn(RxApp.MainThreadScheduler)
			.Select(status => (UseTor, status.IsTorRunning) switch
			{
				(TorMode.Disabled, _) => TorStatus.TurnedOff,
				(_, true) => TorStatus.Running,
				(_, false) => TorStatus.NotRunning
			})
			.BindTo(this, x => x.TorStatus)
			.DisposeWith(Disposables);

		// Tor Issues
		var issues =
			torStatusChecker.Issues
			.Select(r => r.Where(issue => !issue.Resolved).ToList())
			.ObserveOn(RxApp.MainThreadScheduler)
			.Publish();

		_torIssues = issues.ToProperty(this, m => m.TorIssues);

		issues.Connect()
			.DisposeWith(Disposables);

		var nodesCount = 0;
		var peerAddedObservable = services.EventBus.AsObservable<P2pNodeAdded>();
		peerAddedObservable.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(_ => nodesCount++)
			.DisposeWith(Disposables);
		var peerRemovedObservable = services.EventBus.AsObservable<P2pNodeRemoved>();
		peerRemovedObservable.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(_ => nodesCount--)
			.DisposeWith(Disposables);

		// Peers
		Observable.Merge( peerAddedObservable.ToSignal().Merge(peerRemovedObservable.ToSignal())
			.Merge(services.EventBus.AsObservable<TorConnectionStateChanged>().ToSignal()))
			.ObserveOn(RxApp.MainThreadScheduler)
			.Select(_ =>
				  UseTor != TorMode.Disabled && TorStatus == TorStatus.NotRunning ? 0 : nodesCount)
			.BindTo(this, x => x.Peers)
			.DisposeWith(Disposables);

		// Is P2P Connected
		this.WhenAnyValue(x => x.Peers)
			.Select(peerCount => peerCount > 0)
			.BindTo(this, x => x.IsP2pConnected)
			.DisposeWith(Disposables);

		// Update Available
		services.EventBus.AsObservable<NewSoftwareVersionAvailable>()
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(e =>
			{
				var updateStatus = e.UpdateStatus;

				UpdateAvailable = !updateStatus.ClientUpToDate;
				IsReadyToInstall = updateStatus.IsReadyToInstall;
				ClientVersion = updateStatus.ClientVersion;
			})
			.DisposeWith(Disposables);

		// State
		this.WhenAnyValue(
				x => x.TorStatus,
				x => x.Peers,
				x => x.BlockchainTip,
				x => x.ClientTip,
				x => x.UpdateAvailable,
				x => x.CheckForUpdates)
			.Throttle(TimeSpan.FromMilliseconds(100))
			.ObserveOn(RxApp.MainThreadScheduler)
			.Select(_ => GetState())
			.BindTo(this, x => x.State)
			.DisposeWith(Disposables);
	}

	public ICollection<Issue> TorIssues => _torIssues.Value;

	public TorMode UseTor { get; }

	private CompositeDisposable Disposables { get; } = new();

	public void Dispose()
	{
		Disposables.Dispose();
	}

	private HealthMonitorState GetState()
	{
		if (CheckForUpdates && UpdateAvailable)
		{
			return HealthMonitorState.UpdateAvailable;
		}

		var torConnected = UseTor == TorMode.Disabled || TorStatus == TorStatus.Running;

		if (torConnected && IsP2pConnected && BlockchainTip > 0 && BlockchainTip == ClientTip)
		{
			return HealthMonitorState.Ready;
		}
		return HealthMonitorState.Loading;
	}
}
