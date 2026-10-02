using System.Collections.ObjectModel;
using System.Linq;
using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using NBitcoin;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Coinjoins;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.History.Details;

[NavigationMetaData(Title = "Coinjoins", NavigationTarget = NavigationTarget.DialogScreen)]
public partial class CoinJoinsDetailsViewModel : RoutableViewModel
{
	private readonly IWalletModel _wallet;
	private readonly CoinJoinTransactionGroupModel _transaction;

	[AutoNotify] private string _date = "";
	[AutoNotify] private string _status = "";
	[AutoNotify] private uint256? _transactionId;
	[AutoNotify] private ObservableCollection<uint256>? _transactionIds;
	[AutoNotify] private int _txCount;

	public CoinJoinsDetailsViewModel(UiContext uiContext, IWalletModel wallet, CoinJoinTransactionGroupModel transaction) : base(uiContext)
	{
		_wallet = wallet;
		_transaction = transaction;

		Costs = new CoinjoinCostsViewModel(wallet.AmountProvider.Create);

		SetupCancel(enableCancel: false, enableCancelOnEscape: true, enableCancelOnPressed: true);
		NextCommand = CancelCommand;
	}

	public CoinjoinCostsViewModel Costs { get; }

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		base.OnNavigatedTo(isInHistory, disposables);

		_wallet.Transactions.Cache
			                .Connect()
							.Do(_ => Update())
							.Subscribe()
							.DisposeWith(disposables);
	}

	private void Update()
	{
		if (_wallet.Transactions.TryGetById<CoinJoinTransactionGroupModel>(_transaction.Id, out var transaction))
		{
			Date = transaction.DateToolTipString;
			Status = transaction.IsConfirmed ? "Confirmed" : "Pending";
			Costs.Update(transaction.CoinjoinCosts, transaction.Amount);
			TransactionId = transaction.Id;
			TransactionIds = new ObservableCollection<uint256>(transaction.Children.Select(x => x.Id));
			TxCount = TransactionIds.Count;
		}
	}
}
