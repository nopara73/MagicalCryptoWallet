using System.Collections.ObjectModel;
using System.Linq;
using System.Reactive.Linq;
using System.Threading.Tasks;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.NavBar;

[AppLifetime]
public partial class NavBarViewModel : ViewModelBase, IWalletNavigation
{
	[AutoNotify] private WalletPageViewModel? _wallet;

	public NavBarViewModel(UiContext uiContext) : base(uiContext)
	{
		BottomItems = new ObservableCollection<NavBarItemViewModel>();
		UiContext.WalletRepository.WhenAnyValue(x => x.Wallet)
			.WhereNotNull()
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(wallet => Wallet = new WalletPageViewModel(UiContext, wallet));
	}

	public ObservableCollection<NavBarItemViewModel> BottomItems { get; }

	// AutoInterfaces cannot be seen by AutoNotifyGenerator.
	public IWalletModel? WalletModel
	{
		get;
		private set => this.RaiseAndSetIfChanged(ref field, value);
	}

	IWalletViewModel? IWalletNavigation.Wallet => Wallet?.WalletViewModel;

	public void Activate()
	{
		this.WhenAnyValue(x => x.Wallet)
			.WhereNotNull()
			.Subscribe(wallet =>
			{
				WalletModel = wallet.WalletModel;
				wallet.IsSelected = true;
			});
	}

	public async Task InitialiseAsync()
	{
		foreach (var item in NavigationManager.MetaData.Where(x => x.NavBarPosition == NavBarPosition.Bottom))
		{
			var viewModel = await NavigationManager.MaterializeViewModelAsync(item);
			if (viewModel is INavBarItem navBarItem)
			{
				BottomItems.Add(new NavBarItemViewModel(UiContext, navBarItem));
			}
		}
	}

	IWalletViewModel? IWalletNavigation.To(IWalletModel wallet)
	{
		if (Wallet is not { } page || !ReferenceEquals(page.WalletModel, wallet))
		{
			throw new InvalidOperationException("This is not the configured wallet.");
		}
		page.OpenCommand.Execute(default);
		return page.WalletViewModel;
	}
}
