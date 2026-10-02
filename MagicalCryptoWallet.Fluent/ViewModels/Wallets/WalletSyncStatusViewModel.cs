using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets;

[NavigationMetaData(Title = "Synchronization")]
public partial class WalletSyncStatusViewModel : RoutableViewModel, IDisposable
{
	private readonly CompositeDisposable _lifetime = new();
	[AutoNotify] private bool _needsAccountAuthorization;

	[AutoNotify] private string _statusText = "Loading";
	[AutoNotify] private string? _error;
	[AutoNotify] private bool _isFaulted;
	[AutoNotify] private bool _isExpanded;
	[AutoNotify] private uint _latestBlock;

	// Sync progress card models (initial, current, target - percent/isComplete are calculated)
	[AutoNotify] private SyncProgressCardModel _peers = new(0, 0, 12);
	[AutoNotify] private SyncProgressCardModel _blockHeaders = new(0, 0, 0);
	[AutoNotify] private SyncProgressCardModel _filterHeaders = new(0, 0, 0);
	[AutoNotify] private SyncProgressCardModel _compactFilters = new(0, 0, 0);
	[AutoNotify] private SyncProgressCardModel _blocks = new(0, 0, 0);

	public WalletSyncStatusViewModel(UiContext uiContext, IWalletModel wallet) : base(uiContext)
	{
		AuthorizeAccountsCommand = ReactiveCommand.CreateFromTask(async () => { using var scope = await MagicalCryptoWallet.Fluent.Helpers.AuthorizationHelpers.AuthorizeAsync(UiContext, wallet); });
		RetryCommand = ReactiveCommand.CreateFromTask(() => UiContext.Services.WalletSession.RetryAsync());
		wallet.Status.Subscribe(snapshot =>
		{
			StatusText = snapshot.PublicMetadataRequiresAuthorization ? "Additional accounts await authorization" : snapshot.State.ToString();
			Error = snapshot.Error;
			NeedsAccountAuthorization = snapshot.PublicMetadataRequiresAuthorization;
			if (snapshot.State == MagicalCryptoWallet.Wallets.WalletSessionState.Faulted) { IsExpanded = true; }
			IsFaulted = snapshot.State == MagicalCryptoWallet.Wallets.WalletSessionState.Faulted;
		}).DisposeWith(_lifetime);
		wallet.SyncProgress.Progress.Subscribe(UpdateStatus).DisposeWith(_lifetime);
	}

	public System.Windows.Input.ICommand RetryCommand { get; }
	public System.Windows.Input.ICommand AuthorizeAccountsCommand { get; }

	private void UpdateStatus(WalletLoadProgress progress)
	{
		Peers = progress.Peers;
		BlockHeaders = progress.BlockHeaders;
		FilterHeaders = progress.FilterHeaders;
		CompactFilters = progress.CompactFilters;
		Blocks = progress.Blocks;
		LatestBlock = progress.ChainTip;
	}
	public void Dispose() { _lifetime.Dispose();  }

}
