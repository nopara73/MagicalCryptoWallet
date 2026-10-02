using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using System.Windows.Input;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets;

[NavigationMetaData(Title = "Wallet recovery")]
public partial class WalletRecoveryViewModel : RoutableViewModel
{
	[AutoNotify] private string? _error;
	public WalletRecoveryViewModel(UiContext context) : base(context)
	{
		Error = context.Services.WalletSession.Snapshot.Error;
		RetryCommand = ReactiveCommand.CreateFromTask(() => context.Services.WalletSession.RetryAsync());
	}
	public ICommand RetryCommand { get; }
	public string StorageDirectory => UiContext.Services.WalletSession.WalletDirectories.WalletsDir;
	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		Observable.Create<WalletSessionSnapshot>(observer => UiContext.Services.WalletSession.Subscribe(observer.OnNext))
			.ObserveOn(RxApp.MainThreadScheduler).Subscribe(status => Error = status.Error).DisposeWith(disposables);
	}
}
