using System.Collections.Generic;
using System.Linq;
using System.Reactive.Concurrency;
using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Reactive.Linq;
using System.Threading;
using System.Threading.Tasks;
using System.Windows.Input;
using NBitcoin;
using NBitcoin.Policy;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Exceptions;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Models.Transactions;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.Base;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Send;

[NavigationMetaData(Title = "Transaction Preview")]
public partial class TransactionPreviewViewModel : RoutableViewModel
{
	private readonly Stack<(BuildTransactionResult, TransactionInfo)> _undoHistory;
	private WalletAuthorization? _constructionAuthorization;
	private readonly Wallet _wallet;
	private readonly IWalletModel _walletModel;
	private readonly SendFlowModel _sendFlow;
	private TransactionInfo _info;
	private TransactionInfo _currentTransactionInfo;
	private CancellationTokenSource _cancellationTokenSource;
	[AutoNotify] private BuildTransactionResult? _transaction;
	[AutoNotify] private string _nextButtonText;
	[AutoNotify] private TransactionSummaryViewModel? _displayedTransactionSummary;
	[AutoNotify] private bool _canUndo;
	[AutoNotify] private bool _isFeeAdjustable = true;
	[AutoNotify] private string _feeAdjustToolTip = "Change transaction fee or confirmation time";

	public TransactionPreviewViewModel(UiContext uiContext, IWalletModel walletModel, SendFlowModel sendFlow) : base(uiContext)
	{
		_undoHistory = new();
		_wallet = sendFlow.Wallet;
		_walletModel = walletModel;
		_sendFlow = sendFlow;

		_info = _sendFlow.TransactionInfo ?? throw new InvalidOperationException($"Missing required TransactionInfo.");
		_currentTransactionInfo = _info.Clone();
		_cancellationTokenSource = new CancellationTokenSource();

		PrivacySuggestions = new PrivacySuggestionsFlyoutViewModel(uiContext, walletModel, _sendFlow);
		CurrentTransactionSummary = new TransactionSummaryViewModel(uiContext, this, walletModel, _info);
		PreviewTransactionSummary = new TransactionSummaryViewModel(uiContext, this, walletModel, _info, true);

		TransactionSummaries =
		[
			CurrentTransactionSummary,
			PreviewTransactionSummary
		];

		DisplayedTransactionSummary = CurrentTransactionSummary;

		SetupCancel(enableCancel: true, enableCancelOnEscape: true, enableCancelOnPressed: false);
		EnableBack = true;

		if (PreferPsbtWorkflow)
		{
			SkipCommand = ReactiveCommand.CreateFromTask(OnConfirmAsync);
			NextCommand = ReactiveCommand.CreateFromTask(OnExportPsbtAsync);

			_nextButtonText = "Save PSBT file";
		}
		else
		{
			NextCommand = ReactiveCommand.CreateFromTask(OnConfirmAsync);

			_nextButtonText = "Confirm";
		}

		AdjustFeeCommand = ReactiveCommand.CreateFromTask(OnAdjustFeeAsync);

		UndoCommand = ReactiveCommand.Create(
				() =>
				{
					if (_undoHistory.TryPop(out var previous))
					{
						_info = previous.Item2;
						UpdateTransaction(CurrentTransactionSummary, previous.Item1, false);
						CanUndo = _undoHistory.Count != 0;
					}
				});

	}

	public TransactionSummaryViewModel CurrentTransactionSummary { get; }

	public TransactionSummaryViewModel PreviewTransactionSummary { get; }

	public List<TransactionSummaryViewModel> TransactionSummaries { get; }

	public PrivacySuggestionsFlyoutViewModel PrivacySuggestions { get; }

	public bool PreferPsbtWorkflow => _walletModel.Settings.PreferPsbtWorkflow;

	public ICommand AdjustFeeCommand { get; }


	public ICommand UndoCommand { get; }

	private async Task OnExportPsbtAsync()
	{
		if (Transaction is { })
		{
			bool saved = false;
			try
			{
				saved = await TransactionHelpers.ExportTransactionToBinaryAsync(Transaction);
			}
			catch (Exception ex)
			{
				Logger.LogError(ex);
				await ShowErrorAsync("Transaction Export", ex.ToUserFriendlyString(), "Magical Crypto Wallet was unable to export the PSBT.");
			}

			if (saved)
			{
				Navigate().To().Success();
			}
		}
	}

	private void UpdateTransaction(TransactionSummaryViewModel summary, BuildTransactionResult transaction, bool addToUndoHistory = true)
	{
		if (!summary.IsPreview)
		{
			if (addToUndoHistory)
			{
				AddToUndoHistory();
			}

			Transaction = transaction;
			_currentTransactionInfo = _info.Clone();

			UpdateFeeAdjustability();
		}

		summary.UpdateTransaction(transaction, _info);

		DisplayedTransactionSummary = summary;
	}

	private void UpdateFeeAdjustability()
	{
		if (Transaction is null || !_info.IsPayToMany)
		{
			IsFeeAdjustable = true;
			FeeAdjustToolTip = "Change transaction fee or confirmation time";
			return;
		}

		var hasSubtractFee = _info.SubtractFee || _info.AdditionalRecipients.Any(r => r.IsSubtractFee);

		if (hasSubtractFee)
		{
			IsFeeAdjustable = true;
			FeeAdjustToolTip = "Change transaction fee or confirmation time";
			return;
		}

		// Check if the transaction has a change output (a wallet output that isn't a recipient destination)
		var destinationScripts = _info.AllRecipients
			.Select(r => r.Destination.GetScriptPubKey())
			.ToHashSet();
		var hasChange = Transaction.InnerWalletOutputs
			.Any(c => !destinationScripts.Contains(c.ScriptPubKey));

		if (hasChange)
		{
			IsFeeAdjustable = true;
			FeeAdjustToolTip = "Change transaction fee or confirmation time";
		}
		else
		{
			// No change output and no SubtractFee — the fee is fixed to the leftover.
			IsFeeAdjustable = false;
			FeeAdjustToolTip = "Fee adjustment is not available because the difference between your inputs and payments is too small. Go back and adjust amounts or use Max on a recipient.";
		}
	}

	private async Task OnAdjustFeeAsync()
	{
		DialogViewModelBase<FeeRate> feeDialog = _info.IsCustomFeeUsed
			? new CustomFeeRateDialogViewModel(UiContext, _info)
			: new SendFeeViewModel(UiContext, _wallet, _info, false);

		var feeDialogResult = await NavigateDialogAsync(feeDialog, feeDialog.DefaultTarget);

		if (feeDialogResult.Kind == DialogResultKind.Normal &&
			feeDialogResult.Result is { } feeRate &&
			feeRate != _info.FeeRate) // Prevent rebuild if the selected fee did not change.
		{
			_info.FeeRate = feeRate;
			await BuildAndUpdateAsync();
		}
	}

	private async Task BuildAndUpdateAsync()
	{
		var newTransaction = await BuildTransactionAsync();

		if (newTransaction is { })
		{
			UpdateTransaction(CurrentTransactionSummary, newTransaction);
		}
	}

	private async Task<bool> InitialiseTransactionAsync()
	{
		if (_info.FeeRate == FeeRate.Zero)
		{
			var feeDialogResult = await NavigateDialogAsync(new SendFeeViewModel(UiContext, _wallet, _info, true));
			if (feeDialogResult.Kind == DialogResultKind.Normal && feeDialogResult.Result is { } newFeeRate)
			{
				_info.FeeRate = newFeeRate;
			}
			else
			{
				return false;
			}
		}

		if (!_info.Coins.Any())
		{
			var privacyControlDialogResult =
				await NavigateDialogAsync(new PrivacyControlViewModel(UiContext, _wallet, _sendFlow, _info, Transaction?.SpentCoins, isSilent: true));
			if (privacyControlDialogResult.Kind == DialogResultKind.Normal &&
				privacyControlDialogResult.Result is { } coins)
			{
				_info.Coins = coins;
			}
			else if (privacyControlDialogResult.Kind != DialogResultKind.Normal)
			{
				return false;
			}
		}

		return true;
	}

	private async Task<BuildTransactionResult?> BuildTransactionAsync()
	{
		if (!await InitialiseTransactionAsync())
		{
			return null;
		}

		try
		{
			UiContext.Services.WalletSession.EnsureReady();
			var needsSecrets = _info.AllRecipients.Any(recipient => recipient.Destination is Destination.Silent);
			if (needsSecrets && _constructionAuthorization is null)
			{
				_constructionAuthorization = await AuthorizationHelpers.AuthorizeAsync(UiContext, _walletModel);
				if (_constructionAuthorization is null) { return null; }
			}
			IsBusy = true;
			return await Task.Run(() => TransactionHelpers.BuildTransaction(_wallet, _info, tryToSign: needsSecrets, authorization: _constructionAuthorization));
		}
		catch (Exception ex) when (ex is NotEnoughFundsException or TransactionFeeOverpaymentException || (ex is InvalidTxException itx && itx.Errors.OfType<FeeTooHighPolicyError>().Any()))
		{
			if (await TransactionFeeHelper.TrySetMaxFeeRateAsync(UiContext, _wallet, _info))
			{
				return await BuildTransactionAsync();
			}

			await ShowErrorAsync(
				"Transaction Building",
				"The transaction cannot be sent because its fee is more than the payment amount.",
				"Magical Crypto Wallet was unable to create your transaction.");

			return null;
		}
		catch (InsufficientBalanceException)
		{
			var canSelectMoreCoins = _sendFlow.AvailableCoins.Any(coin => !_info.Coins.Contains(coin));

			if (canSelectMoreCoins)
			{
				var selectPocketsDialog =
					await NavigateDialogAsync(new PrivacyControlViewModel(UiContext, _wallet, _sendFlow, _info, usedCoins: Transaction?.SpentCoins, isSilent: true));

				if (selectPocketsDialog.Result is { } newCoins)
				{
					_info.Coins = newCoins;
					return await BuildTransactionAsync();
				}
			}
			else if (await TransactionFeeHelper.TrySetMaxFeeRateAsync(UiContext, _wallet, _info))
			{
				return await BuildTransactionAsync();
			}

			await ShowErrorAsync(
				"Transaction Building",
				"There are not enough funds to cover the transaction fee.",
				"Magical Crypto Wallet was unable to create your transaction.");

			return null;
		}
		catch (Exception ex)
		{
			Logger.LogError(ex);

			await ShowErrorAsync(
				"Transaction Building",
				ex.ToUserFriendlyString(),
				"Magical Crypto Wallet was unable to create your transaction.");

			return null;
		}
		finally
		{
			IsBusy = false;
		}
	}

	private async Task InitialiseViewModelAsync()
	{
		if (await BuildTransactionAsync() is { } initialTransaction)
		{
			UpdateTransaction(CurrentTransactionSummary, initialTransaction);
		}
		else
		{
			Navigate().Back();
		}
	}

	protected override void OnNavigatedTo(bool isInHistory, CompositeDisposable disposables)
	{
		base.OnNavigatedTo(isInHistory, disposables);

		PrivacySuggestions.WhenAnyValue(x => x.PreviewSuggestion)
			.DoAsync(
				async x =>
				{
					if (x?.Transaction is { } transaction)
					{
						UpdateTransaction(PreviewTransactionSummary, transaction);
						await PrivacySuggestions.UpdatePreviewWarningsAsync(_info, transaction, _cancellationTokenSource.Token);
					}
					else
					{
						DisplayedTransactionSummary = CurrentTransactionSummary;
						PrivacySuggestions.ClearPreviewWarnings();
					}
				})
			.Subscribe()
			.DisposeWith(disposables);

		PrivacySuggestions.WhenAnyValue(x => x.SelectedSuggestion)
			.SubscribeAsync(
				async suggestion =>
				{
					PrivacySuggestions.SelectedSuggestion = null;

					if (suggestion is { })
					{
						await ApplyPrivacySuggestionAsync(suggestion);
					}
				})
			.DisposeWith(disposables);

		this.WhenAnyValue(x => x.Transaction)
			.WhereNotNull()
			.Throttle(TimeSpan.FromMilliseconds(100))
			.ObserveOn(RxApp.MainThreadScheduler)
			.Do(
				_ =>
				{
					_cancellationTokenSource.Cancel();
					_cancellationTokenSource = new();
				})
			.DoAsync(
				async transaction =>
				{
					await CheckChangePocketAvailableAsync(transaction);
					await PrivacySuggestions.BuildPrivacySuggestionsAsync(_info, transaction, _cancellationTokenSource.Token);
				})
			.Subscribe()
			.DisposeWith(disposables);

		if (!isInHistory)
		{
			RxApp.MainThreadScheduler.Schedule(async () => await InitialiseViewModelAsync());
		}
	}

	protected override void OnNavigatedFrom(bool isInHistory)
	{
		if (!isInHistory)
		{
			_cancellationTokenSource.Cancel();
			_cancellationTokenSource.Dispose();
			_constructionAuthorization?.Dispose();
			_constructionAuthorization = null;
		}

		base.OnNavigatedFrom(isInHistory);

		DisplayedTransactionSummary = null;
	}

	private async Task OnConfirmAsync()
	{
		try
		{
			UiContext.Services.WalletSession.EnsureReady();
			var transaction = Transaction ?? throw new InvalidOperationException("Review a transaction before sending it.");
			using var transactionAuthorizationInfo = new TransactionAuthorizationInfo(transaction);
			var authResult = _constructionAuthorization is not null && transaction.Signed;
			if (authResult) { transactionAuthorizationInfo.Authorization = _constructionAuthorization!.Retain(); }
			else { authResult = await AuthorizeAsync(transactionAuthorizationInfo); }
			if (authResult)
			{
				IsBusy = true;

				var finalTransaction =
					await GetFinalTransactionAsync(transactionAuthorizationInfo.Transaction, _info, transactionAuthorizationInfo.Authorization);
				await SendTransactionAsync(finalTransaction);
				_wallet.UpdateUsedHdPubKeysLabels(transaction.HdPubKeysWithNewLabels);
				_cancellationTokenSource.Cancel();
				Navigate().To().SendSuccess(finalTransaction);
			}
		}
		catch (Exception ex)
		{
			Logger.LogError(ex);
			await ShowErrorAsync(
				"Transaction",
				ex.ToUserFriendlyString(),
				"Magical Crypto Wallet was unable to send your transaction.");
		}
		finally
		{
			IsBusy = false;
		}
	}

	private Task<bool> AuthorizeAsync(TransactionAuthorizationInfo transactionAuthorizationInfo) =>
		AuthorizationHelpers.AuthorizeTransactionAsync(UiContext, _walletModel, transactionAuthorizationInfo);

	private async Task SendTransactionAsync(SmartTransaction transaction)
	{
		await UiContext.Services.SendTransactionAsync(transaction);
	}

	private async Task<SmartTransaction> GetFinalTransactionAsync(SmartTransaction transaction, TransactionInfo transactionInfo, WalletAuthorization? authorization)
	{
		if (transactionInfo.PayJoinClient is { } && authorization is not null)
		{
			try
			{
				var payJoinTransaction = await Task.Run(() =>
					TransactionHelpers.BuildTransaction(_wallet, transactionInfo, isPayJoin: true, tryToSign: true, authorization: authorization));
				return payJoinTransaction.Transaction;
			}
			catch (Exception ex)
			{
				Logger.LogError(ex);
			}
		}

		return transaction;
	}

	private void AddToUndoHistory()
	{
		if (Transaction is { })
		{
			_undoHistory.Push((Transaction, _currentTransactionInfo));
			CanUndo = true;
		}
	}

	private async Task CheckChangePocketAvailableAsync(BuildTransactionResult transaction)
	{
		var cjManager = UiContext.Services.GetHostedService<CoinJoinManager>();

		var usedCoins = transaction.SpentCoins;
		var pockets = _sendFlow.GetPockets();
		var labelSelection = new LabelSelectionViewModel(UiContext, _wallet.KeyManager, string.Empty, _info, isSilent: true);
		await labelSelection.ResetAsync(pockets, coinsToExclude: cjManager?.CoinsInCriticalPhase.ToList() ?? []);

		_info.IsOtherPocketSelectionPossible = labelSelection.IsOtherSelectionPossible(usedCoins, _info.Recipient);
	}

	private async Task ApplyPrivacySuggestionAsync(PrivacySuggestion suggestion)
	{
		switch (suggestion)
		{
			case LabelManagementSuggestion:
				{
					var selectPocketsDialog =
						await NavigateDialogAsync(new PrivacyControlViewModel(UiContext, _wallet, _sendFlow, _info, Transaction?.SpentCoins, false));

					if (selectPocketsDialog.Kind == DialogResultKind.Normal && selectPocketsDialog.Result is { })
					{
						_info.Coins = selectPocketsDialog.Result;
						await BuildAndUpdateAsync();
					}

					break;
				}

			case ChangeAvoidanceSuggestion { Transaction: { } txn }:
				_info.ChangelessCoins = txn.SpentCoins;
				break;

			case FullPrivacySuggestion fullPrivacySuggestion:
				{
					if (fullPrivacySuggestion.IsChangeless)
					{
						_info.ChangelessCoins = fullPrivacySuggestion.Coins;
					}
					else
					{
						_info.Coins = fullPrivacySuggestion.Coins;
					}

					break;
				}

			case BetterPrivacySuggestion betterPrivacySuggestion:
				{
					if (betterPrivacySuggestion.IsChangeless)
					{
						_info.ChangelessCoins = betterPrivacySuggestion.Coins;
					}
					else
					{
						_info.Coins = betterPrivacySuggestion.Coins;
					}

					break;
				}
		}

		if (suggestion.Transaction is { } transaction)
		{
			if (_info.AllRecipients.Any(recipient => recipient.Destination is Destination.Silent))
			{
				// Silent-payment suggestions must be resolved with this operation's credentials before review.
				await BuildAndUpdateAsync();
			}
			else { UpdateTransaction(CurrentTransactionSummary, transaction); }
		}
	}
}
