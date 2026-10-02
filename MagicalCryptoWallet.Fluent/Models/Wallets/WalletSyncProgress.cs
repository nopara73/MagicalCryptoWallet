using System.Reactive;
using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using System.Reactive.Subjects;
using System.Threading.Tasks;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;


public record SyncProgressCardModel(uint Initial, uint Current, uint Target)
{
	public double Percent =>
		Target == 0 ? (Current == 0 ? 100 : 0)
		: Target <= Initial ? (Current >= Target ? 100 : 0)
		: Math.Clamp(100.0 * ((double)Current - Initial) / (Target - Initial), 0, 100);

	public bool IsComplete => Percent >= 100;
}

public record WalletLoadProgress(
	uint ChainTip,
	SyncProgressCardModel Peers,
	SyncProgressCardModel BlockHeaders,
	SyncProgressCardModel FilterHeaders,
	SyncProgressCardModel CompactFilters,
	SyncProgressCardModel Blocks);

public partial class WalletSyncProgress : IDisposable
{
	private const uint TargetPeers = 12;

	private readonly IServices _services;
	private readonly CompositeDisposable _disposables = new();
	private readonly Wallet _wallet;
	private uint _blockHeadersTip;
	private uint _filterHeadersTip;
	private uint _compactFiltersTip;
	private uint _walletSyncHeight;
	private readonly BehaviorSubject<WalletLoadProgress> _progress;

	// The progress of every stage syncing the wallet is measured from.
	private readonly uint _walletStartHeight;

	public WalletSyncProgress(IServices services, Wallet wallet)
	{
		_services = services;
		_wallet = wallet;
		_progress = new(new(0, new(0,0,12), new(0,0,0), new(0,0,0), new(0,0,0), new(0,0,0)));

		var tipHeight = services.GetTipHeight();
		var walletBestHeight = (uint)wallet.KeyManager.GetBestHeight();

		_blockHeadersTip = services.GetBlockHeadersTipHeight();
		_filterHeadersTip = tipHeight;
		_compactFiltersTip = tipHeight;
		_walletSyncHeight = walletBestHeight;

		// Wallets created before the birth height was introduced don't have one.
		_walletStartHeight = wallet.KeyManager.GetBirthHeight() is { } birthHeight ? (uint)birthHeight : walletBestHeight;

		UpdateProgress();

		services.EventBus.AsObservable<BlockHeadersTipChanged>()
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(x => _blockHeadersTip = x.Height)
			.DisposeWith(_disposables);

		services.EventBus.AsObservable<FilterHeadersTipChanged>()
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(x => _filterHeadersTip = x.Height)
			.DisposeWith(_disposables);

		services.EventBus.AsObservable<ClientTipHeightChanged>()
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(x =>
			{
				_compactFiltersTip = x.Height;
				// Initially the filters' height is initialized but the filter headers' is not
				_filterHeadersTip = uint.Max(_filterHeadersTip, _compactFiltersTip);
			})
			.DisposeWith(_disposables);

		services.EventBus.AsObservable<BlockDownloaded>()
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(x => _walletSyncHeight = x.Height)
			.DisposeWith(_disposables);

		Observable.Create<WalletSessionSnapshot>(observer => services.WalletSession.Subscribe(observer.OnNext))
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(snapshot => { _walletSyncHeight = snapshot.SyncHeight ?? _walletSyncHeight; UpdateProgress(); })
			.DisposeWith(_disposables);
		Observable.Interval(TimeSpan.FromSeconds(1)).ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(_ => UpdateProgress()).DisposeWith(_disposables);
	}

	public IObservable<WalletLoadProgress> Progress => _progress;
	public void Dispose() { _disposables.Dispose(); _progress.Dispose(); }

	private void UpdateProgress()
	{
		var serverTipHeight = _services.GetServerTipHeight();
		var clientTipHeight = _services.GetTipHeight();

		var tipHeight = Math.Max(serverTipHeight, clientTipHeight);

		_progress.OnNext(new WalletLoadProgress(
			ChainTip: tipHeight,
			Peers: new SyncProgressCardModel(0, (uint)_services.GetPeerCount(), TargetPeers),

			// Block headers are synced for the whole chain, not only for the range the wallet is scanned in.
			BlockHeaders: new SyncProgressCardModel(0, _blockHeadersTip, tipHeight),

			FilterHeaders: new SyncProgressCardModel(_walletStartHeight, _filterHeadersTip, tipHeight),
			CompactFilters: new SyncProgressCardModel(_walletStartHeight, _compactFiltersTip, tipHeight),
			Blocks: new SyncProgressCardModel(_walletStartHeight, _walletSyncHeight, tipHeight)));
	}
}
