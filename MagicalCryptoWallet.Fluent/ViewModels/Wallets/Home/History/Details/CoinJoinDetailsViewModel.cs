using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using NBitcoin;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Fluent.ViewModels.Wallets.Coinjoins;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Home.History.Details;

[NavigationMetaData(Title = "Coinjoin Details", NavigationTarget = NavigationTarget.DialogScreen)]
public partial class CoinJoinDetailsViewModel : RoutableViewModel
{
	private readonly IWalletModel _wallet;
	private readonly CoinJoinTransactionModel _transaction;

	[AutoNotify] private string _date = "";
	[AutoNotify] private uint256? _transactionId;
	[AutoNotify] private bool _isConfirmed;
	[AutoNotify] private uint _confirmations;

	public CoinJoinDetailsViewModel(UiContext uiContext, IWalletModel wallet, CoinJoinTransactionModel transaction) : base(uiContext)
	{
		_wallet = wallet;
		_transaction = transaction;

		Costs = new CoinjoinCostsViewModel(wallet.AmountProvider.Create);

		TransactionHex = transaction.Hex.Value;

		SetupCancel(enableCancel: false, enableCancelOnEscape: true, enableCancelOnPressed: true);
		NextCommand = CancelCommand;
	}

	public CoinjoinCostsViewModel Costs { get; }
	public string TransactionHex { get; }

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		base.OnNavigatedTo(isInHistory, disposables);

		_wallet.Transactions.Cache
							.Connect()
							.ObserveOn(RxApp.MainThreadScheduler)
							.Subscribe(_ => Update())
							.DisposeWith(disposables);
	}

	private void Update()
	{
		if (_wallet.Transactions.TryGetById<CoinJoinTransactionModel>(_transaction.Id, out var transaction))
		{
			Date = transaction.DateToolTipString;
			Costs.Update(transaction.CoinjoinCosts, transaction.Amount);
			Confirmations = transaction.Confirmations;
			IsConfirmed = Confirmations > 0;
			TransactionId = transaction.Id;
		}
	}
}
