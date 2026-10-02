using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
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
public partial class NavBarViewModel : ViewModelBase, IWalletNavigation, IDisposable
{
	private readonly CompositeDisposable _lifetime = new();
	[AutoNotify] private WalletViewModel? _home;

	public NavBarViewModel(UiContext uiContext) : base(uiContext)
	{
		BottomItems = new ObservableCollection<NavBarItemViewModel>();
		UiContext.WalletSetupService.WhenAnyValue(x => x.Wallet)
			.WhereNotNull()
			.ObserveOn(RxApp.MainThreadScheduler)
			.Subscribe(wallet => { Home?.Dispose(); Home = new WalletViewModel(UiContext, wallet); }).DisposeWith(_lifetime);
	}

	public ObservableCollection<NavBarItemViewModel> BottomItems { get; }

	public void Activate()
	{
		this.WhenAnyValue(x => x.Home).WhereNotNull().Subscribe(_ => OpenWalletHome()).DisposeWith(_lifetime);
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

	public IWalletViewModel? OpenWalletHome()
	{
		if (Home is { } home) { UiContext.Navigate().To(home, NavigationTarget.HomeScreen, NavigationMode.Clear); }
		return Home;
	}
	public void Dispose() { _lifetime.Dispose(); Home?.Dispose(); }

}
