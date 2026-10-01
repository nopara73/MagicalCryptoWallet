using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets;

[NavigationMetaData(Title = "")]
public partial class LoadingViewModel : RoutableViewModel
{
	private readonly IWalletModel _wallet;

	[AutoNotify] private bool _isLoading;
	[AutoNotify] private uint _latestBlock;

	// Sync progress card models (initial, current, target - percent/isComplete are calculated)
	[AutoNotify] private SyncProgressCardModel _peers = new(0, 0, 12);
	[AutoNotify] private SyncProgressCardModel _blockHeaders = new(0, 0, 0);
	[AutoNotify] private SyncProgressCardModel _filterHeaders = new(0, 0, 0);
	[AutoNotify] private SyncProgressCardModel _compactFilters = new(0, 0, 0);
	[AutoNotify] private SyncProgressCardModel _blocks = new(0, 0, 0);

	public LoadingViewModel(UiContext uiContext, IWalletModel wallet) : base(uiContext)
	{
		_wallet = wallet;
	}

	public string WalletName => _wallet.Name;

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		_wallet.Loader.Progress
			.Do(p => UpdateStatus(p))
			.Subscribe()
			.DisposeWith(disposables);
	}

	private void UpdateStatus(WalletLoadProgress progress)
	{
		Peers = progress.Peers;
		BlockHeaders = progress.BlockHeaders;
		FilterHeaders = progress.FilterHeaders;
		CompactFilters = progress.CompactFilters;
		Blocks = progress.Blocks;
		LatestBlock = progress.ChainTip;
	}
}
