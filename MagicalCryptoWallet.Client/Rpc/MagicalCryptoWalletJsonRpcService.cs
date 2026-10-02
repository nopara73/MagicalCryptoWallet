using System;
using System.Collections.Generic;
using System.Collections.Immutable;
using System.Diagnostics.CodeAnalysis;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Rpc;
using MagicalCryptoWallet.WabiSabi.Client.Batching;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;
using MagicalCryptoWallet.Wallets;
using JsonRpcResult = System.Collections.Generic.Dictionary<string, object?>;
using JsonRpcResultList = System.Collections.Immutable.ImmutableArray<System.Collections.Generic.Dictionary<string, object?>>;

namespace MagicalCryptoWallet.Client.Rpc;

public class MagicalCryptoWalletJsonRpcService : IJsonRpcService
{
	public MagicalCryptoWalletJsonRpcService(Global global)
	{
		Global = global;
	}

	private Global Global { get; }
	private Wallet? ActiveWallet => Global?.WalletSession.GetWallet();

	[JsonRpcMethod("listunspentcoins")]
	public JsonRpcResultList GetUnspentCoinList()
	{
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);

		AssertCachedData();
		var serverTipHeight = Global.FilterHeaders.ServerTipHeight;
		return activeWallet.Coins.Where(x => !x.IsSpent()).Select(
			x => new JsonRpcResult
			{
				["txid"] = x.TransactionId.ToString(),
				["index"] = x.Index,
				["amount"] = x.Amount.Satoshi,
				["anonymityScore"] = x.AnonymitySet,
				["confirmed"] = x.Confirmed,
				["confirmations"] = x.Transaction.GetConfirmations(serverTipHeight),
				["label"] = x.HdPubKey.Labels.ToString(),
				["keyPath"] = x.HdPubKey.FullKeyPath.ToString(),
				["address"] = x.HdPubKey.GetAddress(Global.Network).ToString(),
			}).ToImmutableArray();
	}

	[JsonRpcMethod("listcoins")]
	public JsonRpcResultList GetCoinList()
	{
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);

		AssertCachedData();
		var serverTipHeight = Global.FilterHeaders.ServerTipHeight;
		if (activeWallet.Coins is not { } coinRegistry)
		{
			throw new ArgumentException($"{nameof(activeWallet.Coins)} was not {typeof(CoinsRegistry)}.");
		}
		return coinRegistry.AsAllCoinsView().Select(
			x => new JsonRpcResult
			{
				["txid"] = x.TransactionId.ToString(),
				["index"] = x.Index,
				["amount"] = x.Amount.Satoshi,
				["anonymityScore"] = x.AnonymitySet,
				["confirmed"] = x.Confirmed,
				["confirmations"] = x.Transaction.GetConfirmations(serverTipHeight),
				["keyPath"] = x.HdPubKey.FullKeyPath.ToString(),
				["address"] = x.HdPubKey.GetAddress(Global.Network).ToString(),
				["spentBy"] = x.SpenderTransaction?.GetHash().ToString()
			}).ToImmutableArray();
	}

	[JsonRpcMethod("createwallet", initializable: false)]
	public object CreateWallet(string password)
	{
		Global.WalletSession.EnsureCanConfigure();
		var walletGenerator = new WalletGenerator(Global.WalletSession.WalletDirectories.WalletsDir, Global.Network);
		walletGenerator.TipHeight = Global.FilterHeaders.TipHeight;
		var (keyManager, mnemonic) = walletGenerator.GenerateDraft(password, mnemonic: null);
		Global.WalletSession.Configure(keyManager);
		return mnemonic.ToString();
	}

	[JsonRpcMethod("recoverwallet", initializable: false)]
	public void RecoverWallet(string mnemonicStr, string password = "")
	{
		Global.WalletSession.EnsureCanConfigure();
		var walletGenerator = new WalletGenerator(Global.WalletSession.WalletDirectories.WalletsDir, Global.Network);
		walletGenerator.TipHeight = 0;
		if (!TryParseMnemonic(mnemonicStr, out var mnemonic))
		{
			throw new ArgumentException("Invalid value for mnemonic");
		}

		var (keyManager, _) = walletGenerator.GenerateDraft(password, mnemonic);
		Global.WalletSession.Configure(keyManager);
	}

	[JsonRpcMethod("getwalletinfo", initializable: false)]
	public JsonRpcResult WalletInfo()
	{
		var session = Global.WalletSession.Snapshot;
		var info = new JsonRpcResult
		{
			["state"] = session.State.ToString(),
			["hasCachedData"] = session.HasCachedData,
			["synchronized"] = session.IsSynchronized,
			["syncHeight"] = session.SyncHeight,
			["targetHeight"] = session.TargetHeight,
			["coinJoinRequiresAuthorization"] = session.CoinJoinRequiresAuthorization,
			["publicMetadataRequiresAuthorization"] = session.PublicMetadataRequiresAuthorization,
			["error"] = session.Error,
			["balance"] = null,
			["accounts"] = Array.Empty<object>()
		};
		if (ActiveWallet is not { } activeWallet) { return info; }

		var km = activeWallet.KeyManager;
		var segwit = new JsonRpcResult
		{
			["name"] = "segwit",
			["publicKey"] = km.SegwitExtPubKey.ToString(Global.Network),
			["keyPath"] = $"m/{km.SegwitAccountKeyPath}"
		};
		info["walletFile"] = km.FilePath;
		info["masterKeyFingerprint"] = km.MasterFingerprint?.ToString();
		info["anonScoreTarget"] = activeWallet.AnonScoreTarget;
		info["isWatchOnly"] = km.IsWatchOnly;
		info["isHardwareWallet"] = km.IsHardwareWallet;
		info["isAutoCoinjoin"] = km.AutoCoinJoin;
		info["isNonPrivateCoinIsolation"] = km.NonPrivateCoinIsolation;
		info["onlyUsePrivateFundsForPayments"] = km.OnlyUsePrivateFundsForPayments;
		info["accounts"] = new[] { segwit };

		if (km.TaprootExtPubKey is { } taprootExtPubKey)
		{
			info["accounts"] = new[]
			{
				segwit,
				new JsonRpcResult
				{
					["name"] = "taproot",
					["publicKey"] = taprootExtPubKey.ToString(Global.Network),
					["keyPath"] = $"m/{km.TaprootAccountKeyPath}"
				}
			};
		}

		if (session.HasCachedData)
		{
			// Public balances are cached until synchronized is true.
			info["balance"] = activeWallet.Coins
				.Where(c => !c.IsSpent())
				.Sum(c => c.Amount.Satoshi);
			info["coinjoinStatus"] = GetCoinjoinStatus();
		}

		return info;
	}

	[JsonRpcMethod("getnewaddress")]
	public JsonRpcResult GenerateReceiveAddress(string label, bool taproot = true)
	{
		if (!Global.WalletSession.Snapshot.HasCachedData) { throw new InvalidOperationException("Public wallet data is not ready."); }
		label = Guard.NotNullOrEmptyOrWhitespace(nameof(label), label, true);
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);

		var hdKey = taproot
			? activeWallet.KeyManager.GetNextReceiveKey(new LabelsArray(label), ScriptPubKeyType.TaprootBIP86)
			: activeWallet.KeyManager.GetNextReceiveKey(new LabelsArray(label));

		return new JsonRpcResult
		{
			["address"] = hdKey.GetAddress(Global.Network).ToString(),
			["keyPath"] = hdKey.FullKeyPath.ToString(),
			["label"] = hdKey.Labels.ToString(),
			["publicKey"] = hdKey.PubKey.ToHex(),
			["scriptPubKey"] = hdKey.GetAssumedScriptPubKey().ToHex()
		};
	}

	[JsonRpcMethod("getstatus", initializable: false)]
	public JsonRpcResult GetStatus()
	{
		var smartHeaderChain = Global.FilterHeaders;

		return new JsonRpcResult
		{
			["torStatus"] = (Global.Config.UseTor, Global.Status.IsTorRunning) switch
			{
				(TorMode.Disabled, _) => "Turned off",
				(_, true) => "Running",
				(_, false) => "Not running"
			},
			["onionService"] = Global.OnionServiceUri?.ToString() ?? "Unavailable",
			["bestBlockchainHeight"] = smartHeaderChain.TipHeight.ToString(),
			["bestBlockchainHash"] = smartHeaderChain.TipHash?.ToString() ?? "",
			["filtersCount"] = smartHeaderChain.HashCount,
			["filtersLeft"] = smartHeaderChain.HashesLeft,
			["network"] = Global.Network.Name,
			["exchangeRate"] = Global.Status.UsdExchangeRate,
			["peers"] = Global.GetNodes().Select(
				x => new JsonRpcResult
				{
					["isConnected"] = x.IsConnected,
					["lastSeen"] = x.LastSeen,
					["endpoint"] = x.Peer.Endpoint.ToString(),
					["userAgent"] = x.PeerVersion.UserAgent,
				}).ToArray(),
		};
	}

	[JsonRpcMethod("build")]
	public string BuildTransaction(PaymentInfo[] payments, int? feeTarget = null, decimal? feeRate = null, string? password = null)
	{
		Guard.NotNull(nameof(payments), payments);
		password = Guard.Correct(password);

		var feeStrategy = GetFeeStrategy(feeTarget, feeRate);

		AssertWalletReady();
		var payment = new PaymentIntent(
			payments.Select(
				p =>
				new DestinationRequest(p.Sendto, MoneyRequest.Create(p.Amount, p.SubtractFee), new LabelsArray(p.Label))));
		using var authorization = Authorize(password);
		var result = ActiveWallet!.BuildTransaction(
			password,
			payment,
			feeStrategy,
			allowUnconfirmed: true, authorization: authorization);
		var smartTx = result.Transaction;

		return smartTx.Transaction.ToHex();
	}

	/// <summary>
	/// Unsafe, because no matter how big fee the user chooses, MagicalCryptoWallet will build the transaction.
	/// Potentially, the user can burn his money using this method, so be careful!
	/// </summary>
	[JsonRpcMethod("buildunsafetransaction")]
	public string BuildUnsafeTransaction(PaymentInfo[] payments, int? feeTarget = null, decimal? feeRate = null, string? password = null)
	{
		Guard.NotNull(nameof(payments), payments);
		password = Guard.Correct(password);

		var feeStrategy = GetFeeStrategy(feeTarget, feeRate);

		AssertWalletReady();
		var payment = new PaymentIntent(
			payments.Select(
				p =>
				new DestinationRequest(p.Sendto, MoneyRequest.Create(p.Amount, p.SubtractFee), new LabelsArray(p.Label))));
		using var authorization = Authorize(password);
		var result = ActiveWallet!.BuildTransactionWithoutOverpaymentProtection(
			password,
			payment,
			feeStrategy,
			allowUnconfirmed: true, authorization: authorization);
		var smartTx = result.Transaction;

		return smartTx.Transaction.ToHex();
	}

	[JsonRpcMethod("payincoinjoin")]
	public string PayInCoinJoin(BitcoinAddress address, Money amount, string? password = null)
	{
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);
		AssertWalletReady();
		using var authorization = Authorize(password ?? "");
		return activeWallet.AddCoinJoinPayment(address, amount);
	}

	[JsonRpcMethod("listpaymentsincoinjoin")]
	public JsonRpcResultList ListPaymentsInCoinJoin()
	{
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);
		AssertWalletReady();
		var payments = activeWallet.BatchedPayments.GetPayments();
		return payments.Select(x =>
		{
			var paymentResult = new JsonRpcResult
			{
				["id"] = x.Id,
				["amount"] = x.Amount.Satoshi,
				["destination"] = x.Destination.ScriptPubKey.ToHex()
			};

			var state = x.State;
			var stateHistory = new List<JsonRpcResult>();
			while (state != null)
			{
				switch (state)
				{
					case PendingPayment pending:
						stateHistory.Add(new JsonRpcResult
						{
							["status"] = "Pending"
						});
						break;

					case InProgressPayment inProgress:
						stateHistory.Add(new JsonRpcResult
						{
							["status"] = "In progress",
							["round"] = inProgress.RoundId.ToString()
						});
						break;

					case SignedUnknownPayment signed:
						stateHistory.Add(new JsonRpcResult
						{
							["status"] = "Signed",
							["txid"] = signed.TransactionId.ToString()
						});
						break;

					case FinishedPayment finished:
						stateHistory.Add(new JsonRpcResult
						{
							["status"] = "Finished",
							["txid"] = finished.TransactionId.ToString()
						});
						break;

					default:
						throw new NotSupportedException($"Unrecognized state: {state.GetType().Name}.");
				}

				state = state.PreviousState;
			}

			paymentResult["state"] = stateHistory;

			if (x.Destination.ScriptPubKey.GetDestinationAddress(activeWallet.Network) is { } address)
			{
				paymentResult["address"] = address;
			}
			return paymentResult;
		}).ToImmutableArray();
	}

	[JsonRpcMethod("cancelpaymentincoinjoin")]
	public void CancelPayment(Guid paymentId, string password = "")
	{
		AssertWalletReady();
		using var authorization = Authorize(password);
		ActiveWallet!.CancelCoinJoinPayment(paymentId);
	}

	[JsonRpcMethod("send")]
	public async Task<JsonRpcResult> SendTransactionAsync(PaymentInfo[] payments, int? feeTarget = null, int? feeRate = null, string? password = null)
	{
		password = Guard.Correct(password);
		Global.WalletSession.EnsureReady();
		var manager = Global.HostedServices.GetOrDefault<CoinJoinManager>();
		manager?.WalletEnteredSendWorkflow();
		try
		{
			if (manager is not null) { await manager.WalletEnteredSendingAsync().ConfigureAwait(false); }
			Global.WalletSession.EnsureReady();
			var txHex = BuildTransaction(payments, feeTarget, feeRate, password);
			var smartTx = new SmartTransaction(Transaction.Parse(txHex, Global.Network), Height.Mempool);
			await Global.TransactionBroadcaster.SendTransactionAsync(smartTx).ConfigureAwait(false);
			return new JsonRpcResult
			{
				["txid"] = smartTx.Transaction.GetHash(),
				["tx"] = txHex
			};
		}
		finally { manager?.WalletLeftSendWorkflow(); }
	}

	[JsonRpcMethod("canceltransaction")]
	public string BuildCancelTransaction(uint256 txId, string password = "")
	{
		Guard.NotNull(nameof(txId), txId);
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);
		AssertWalletReady();
		using var authorization = Authorize(password);
		var mempoolStore = Global.TransactionStore.MempoolStore;
		if (!mempoolStore.TryGetTransaction(txId, out var smartTransactionToCancel))
		{
			throw new NotSupportedException($"Unknown transaction {txId}");
		}

		var cancellationResult = activeWallet.CancelTransaction(smartTransactionToCancel, authorization);
		var cancellationSmartTransaction = cancellationResult.Transaction;
		return cancellationSmartTransaction.Transaction.ToHex();
	}

	[JsonRpcMethod("speeduptransaction")]
	public async Task<string> SpeedUpTransactionAsync(uint256 txId, string password = "")
	{
		Guard.NotNull(nameof(txId), txId);
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);
		AssertWalletReady();
		using var authorization = Authorize(password);
		var mempoolStore = Global.TransactionStore.MempoolStore;
		if (!mempoolStore.TryGetTransaction(txId, out var smartTransactionToSpeedUp))
		{
			throw new NotSupportedException($"Unknown transaction {txId}");
		}

		var speedUpResult = await activeWallet.SpeedUpTransactionAsync(smartTransactionToSpeedUp, null, CancellationToken.None, authorization).ConfigureAwait(false);
		var speedUpSmartTransaction = speedUpResult.Transaction;
		return speedUpSmartTransaction.Transaction.ToHex();
	}

	[JsonRpcMethod("broadcast", initializable: false)]
	public async Task<JsonRpcResult> SendRawTransactionAsync(string txHex)
	{
		txHex = Guard.Correct(txHex);
		var smartTx = new SmartTransaction(Transaction.Parse(txHex, Global.Network), Height.Mempool);

		await Global.TransactionBroadcaster.SendTransactionAsync(smartTx).ConfigureAwait(false);
		return new JsonRpcResult
		{
			["txid"] = smartTx.Transaction.GetHash()
		};
	}

	[JsonRpcMethod("gethistory")]
	public async Task<JsonRpcResultList> GetHistoryAsync()
	{
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);

		AssertCachedData();
		var summary = await activeWallet.BuildHistorySummaryAsync();
		return summary.Select(
			x => new JsonRpcResult
			{
				["datetime"] = x.FirstSeen,
				["height"] =  x.Height.ToString(),
				["amount"] = x.Amount.Satoshi,
				["label"] = x.Labels.ToString(),
				["tx"] = x.GetHash(),
				["islikelycoinjoin"] = x.IsOwnCoinjoin()
			}).ToImmutableArray();
	}

	[JsonRpcMethod("listkeys")]
	public JsonRpcResultList GetAllKeys()
	{
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);

		AssertCachedData();
		var keys = activeWallet.KeyManager.GetKeys();
		return keys.Select(
			x => new JsonRpcResult
			{
				["fullKeyPath"] = x.FullKeyPath.ToString(),
				["internal"] = x.IsInternal,
				["keyState"] = x.KeyState,
				["label"] = x.Labels.ToString(),
				["scriptPubKey"] = x.GetAssumedScriptPubKey().ToString(),
				["pubkey"] = x.PubKey.ToString(),
				["pubKeyHash"] = x.PubKey.Hash.ToString(),
				["address"] = x.GetAddress(Global.Network).ToString()
			}).ToImmutableArray();
	}

	[JsonRpcMethod("startcoinjoin")]
	public void StartCoinJoining(string? password = null, bool stopWhenAllMixed = true, bool overridePlebStop = true)
	{
		var coinJoinManager = GetCoinJoinManager();
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);

		AssertWalletReady();
		if (password is not null || Global.WalletSession.Snapshot.CoinJoinRequiresAuthorization)
		{
			using var authorization = Authorize(password ?? "");
			Global.WalletSession.AuthorizeCoinJoin(authorization);
		}
		coinJoinManager.RequestCoinJoinStart(stopWhenAllMixed, overridePlebStop);
	}

	[JsonRpcMethod("stopcoinjoin")]
	public void StopCoinJoining()
	{
		var coinJoinManager = GetCoinJoinManager();
		var activeWallet = Guard.NotNull(nameof(ActiveWallet), ActiveWallet);

		AssertWalletReady();

		coinJoinManager.RequestCoinJoinStop();
	}

	[JsonRpcMethod("getfeerates", initializable: false)]
	public object GetFeeRate()
	{
		if (Global.Status.FeeRates is { } nonNullFeeRates)
		{
			return nonNullFeeRates.Estimations;
		}

		return new Dictionary<int, int>();
	}

	[JsonRpcMethod("query", initializable: false)]
	public async Task<object> ExecuteAsync(string script)
	{
		if (!Global.Config.ExperimentalFeatures.Contains("scripting", StringComparer.InvariantCultureIgnoreCase))
		{
			throw new InvalidOperationException("The experimental 'scripting' feature is not enabled.");
		}
		try
		{
			var expressionResult = await Global.Scheme.ExecuteAsync(script).ConfigureAwait(false);
			var result = Scheme.ToObject(Scheme.ToNativeObject(expressionResult));
			return result;
		}
		catch (Exception e)
		{
			return e.Message;
		}

	}

	[JsonRpcMethod(IJsonRpcService.StopRpcCommand, initializable: false)]
	public Task StopAsync()
	{
		throw new InvalidOperationException("This RPC method is special and the handling method should not be called.");
	}

	private CoinJoinManager GetCoinJoinManager()
	{
		var coinJoinManager = Global.HostedServices.GetOrDefault<CoinJoinManager>()
			?? throw new InvalidOperationException("No coordinator configured.");

		return coinJoinManager;
	}

	private string GetCoinjoinStatus()
	{
		var walletCoinjoinClientState = Global.HostedServices.GetOrDefault<CoinJoinManager>()?.ClientState ?? CoinJoinClientState.Idle;
		return walletCoinjoinClientState switch
		{
			CoinJoinClientState.Idle => "Idle",
			CoinJoinClientState.InProgress => "In progress",
			CoinJoinClientState.InSchedule => "In schedule",
			CoinJoinClientState.InCriticalPhase => "In critical phase",
			_ => throw new Exception($"The state {walletCoinjoinClientState.FriendlyName()} is unknown.")
		};
	}

	private void AssertCachedData()
	{
		if (!Global.WalletSession.Snapshot.HasCachedData) { throw new InvalidOperationException("Public wallet data is not ready."); }
	}

	private void AssertWalletReady() => (Global?.WalletSession ?? throw new InvalidOperationException("The wallet session has not initialized.")).EnsureReady();

	private WalletAuthorization Authorize(string password)
	{
		var scope = WalletAuthorization.Create(ActiveWallet?.KeyManager ?? throw new InvalidOperationException("No wallet is configured."), password);
		try { Global.WalletSession.CompleteOperationAuthorization(scope); return scope; }
		catch { scope.Dispose(); throw; }
	}

	[JsonRpcInitialization]
	public void Initialize(string path, bool needsWallet)
	{
		if (path != "/" && path != "")
		{
			throw new InvalidOperationException("Wallet-specific RPC paths are not supported. Use the root endpoint.");
		}
		if (needsWallet && ActiveWallet is null)
		{
			throw new InvalidOperationException("No wallet is configured.");
		}
	}

	private static bool TryParseMnemonic(string mnemonicStr, [NotNullWhen(true)] out Mnemonic? mnemonic)
	{
		try
		{
			mnemonic = new Mnemonic(mnemonicStr);
			return true;
		}
		catch (Exception)
		{
			mnemonic = null;
			return false;
		}
	}

	private FeeStrategy GetFeeStrategy(int? feeTarget = null, decimal? feeRate = null)
	{
		static bool InRange<T>(IComparable<T> val, T min, T max) =>
			val.CompareTo(min) >= 0 && val.CompareTo(max) <= 0;

		var satsPerByte = feeRate is { } nonNullSatsPerByte ? new FeeRate(nonNullSatsPerByte) : FeeRate.Zero;

		return (feeRate, feeTarget) switch
		{
			(not null, null) when InRange(satsPerByte, Constants.MinRelayFeeRate, Constants.AbsurdlyHighFeeRate) =>
				FeeStrategy.CreateFromFeeRate(satsPerByte),
			(null, { } argFeeTarget) when InRange(argFeeTarget, Constants.TwentyMinutesConfirmationTarget, Constants.SevenDaysConfirmationTarget) =>
				FeeStrategy.CreateFromConfirmationTarget(argFeeTarget),
			_ => throw new ArgumentException("Fee parameters are missing, inconsistent or out of range.")
		};
	}
}
