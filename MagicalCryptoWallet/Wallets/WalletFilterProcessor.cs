using Microsoft.Extensions.Hosting;
using NBitcoin;
using Nito.AsyncEx;
using System.Collections.Generic;
using System.IO;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Backend.Models;
using MagicalCryptoWallet.Blockchain.Blocks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionProcessing;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Services.Terminate;
using MagicalCryptoWallet.Stores;
using MagicalCryptoWallet.Wallets.FilterProcessor;

namespace MagicalCryptoWallet.Wallets;

public class WalletFilterProcessor : BackgroundService
{
	public WalletFilterProcessor(
		KeyManager keyManager,
		AllTransactionStore transactionStore,
		FilterStore filterStore,
		FilterHeaderChain filterHeaderChain,
		TransactionProcessor transactionProcessor,
		BlockProvider blockProvider,
		EventBus eventBus)
	{
		_keyManager = keyManager;
		_transactionStore = transactionStore;
		_filterHeaderChain = filterHeaderChain;
		_transactionProcessor = transactionProcessor;
		_blockProvider = blockProvider;
		_eventBus = eventBus;
		_blockFilterIterator = new(filterStore);
		_initialSynchronizationFinished = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
	}

	private readonly KeyManager _keyManager;
	private readonly AllTransactionStore _transactionStore;
	private readonly FilterHeaderChain _filterHeaderChain;
	private readonly TransactionProcessor _transactionProcessor;
	private readonly BlockProvider _blockProvider;
	private readonly EventBus _eventBus;
	private readonly BlockFilterIterator _blockFilterIterator;
	private readonly TaskCompletionSource _initialSynchronizationFinished;

	private bool _waitingForBlock;
	private uint? _replayThroughHeight;
	private long _rescanHeight = -1;
	private bool _rescanPending;
	private readonly Lock _rescanGate = new();
	public bool RescanPending { get { lock (_rescanGate) { return _rescanPending; } } }
	public bool WaitingForBlock => Volatile.Read(ref _waitingForBlock);
	public Task InitialSynchronizationFinished => _initialSynchronizationFinished.Task;

	/// <summary>Make sure we don't process any request while a reorg is happening.</summary>
	private readonly AsyncLock _reorgLock = new();

	private IDisposable? _chainReorgSubscription;
	private readonly Lock _reorgTaskGate = new();
	private Task _pendingReorgs = Task.CompletedTask;
	private bool _acceptReorgs;

	/// <inheritdoc />
	/// <summary>Used for filter synchronization.</summary>
	protected override async Task ExecuteAsync(CancellationToken cancellationToken)
	{
		try
		{
			await Task.WaitForAsync(() => _filterHeaderChain is {Tip: not null, HashesLeft: < 100}, cancellationToken).ConfigureAwait(false);
			var firstSupportedHeight = Blockchain.BlockFilters.FilterCheckpoints.GetCheckpointsByNetwork(_keyManager.GetNetwork())[0].Header.Height;

			while (!cancellationToken.IsCancellationRequested)
			{
				using (await _reorgLock.LockAsync(cancellationToken).ConfigureAwait(false))
				{
					long rescanHeight;
					lock (_rescanGate) { rescanHeight = _rescanHeight; _rescanHeight = -1; }
					if (rescanHeight >= 0) { _replayThroughHeight = _filterHeaderChain.TipHeight; _keyManager.SetMaxBestHeight((uint)rescanHeight); }
					var lastHeight = _keyManager.GetBestHeight();
					if (firstSupportedHeight > 0 && lastHeight < firstSupportedHeight - 1)
					{
						_keyManager.SetBestHeight(firstSupportedHeight - 1);
						lastHeight = _keyManager.GetBestHeight();
					}
					_replayThroughHeight ??= _filterHeaderChain.TipHeight;

					if (lastHeight == _filterHeaderChain.TipHeight)
					{
						lock (_rescanGate) { if (_rescanHeight < 0) { _rescanPending = false; } }
						_initialSynchronizationFinished.TrySetResult();
						await Task.Delay(1_000, cancellationToken).ConfigureAwait(false);
						continue;
					}

					var currentHeight = lastHeight + 1;
					var filter = await _blockFilterIterator.GetAndRemoveAsync(currentHeight, cancellationToken).ConfigureAwait(false);
					if (filter is null)
					{
						// The wallet being processed had been synchronized until a blockchain height which is higher
						// than the top filters that MagicalCryptoWallet has received. That means that the filters were reset, or
						// the wallet was copied and pasted from a more updated setup.
						// Wait for the index store to catch up.
						await Task.Delay(2_000, cancellationToken).ConfigureAwait(false);
						continue;
					}
					bool matchFound;
					try
					{
						matchFound = await ProcessFilterModelAsync(filter, cancellationToken).ConfigureAwait(false);
						Volatile.Write(ref _waitingForBlock, false);
					}
					catch (Exception ex) when (ex is IOException or HttpRequestException or TimeoutException || ex is OperationCanceledException && !cancellationToken.IsCancellationRequested)
					{
						// Keep the height unchanged so the next attempt retries the same filter.
						Volatile.Write(ref _waitingForBlock, true);
						Logger.LogDebug(ex);
						await Task.Delay(2_000, cancellationToken).ConfigureAwait(false);
						continue;
					}
					_eventBus.Publish(new FilterProcessed(filter));

					var reachedBlockChainTip = currentHeight == _filterHeaderChain.TipHeight;
					bool storeToDisk = matchFound || reachedBlockChainTip;
					_keyManager.SetBestHeight(currentHeight, storeToDisk);
				}
			}
		}
		catch (OperationCanceledException) when (cancellationToken.IsCancellationRequested)
		{
			_initialSynchronizationFinished.TrySetCanceled(cancellationToken);
			Logger.LogDebug("Filter processor's execution was stopped.");
		}
		catch (Exception ex)
		{
			_initialSynchronizationFinished.TrySetException(ex);
			Logger.LogError(ex);
			throw;
		}
	}

	private async Task<bool> ProcessFilterModelAsync(FilterModel filter, CancellationToken cancellationToken)
	{
		var toTestKeys = _keyManager.UnsafeGetSynchronizationInfos();

		var matchFound = false;
		if (toTestKeys.Length != 0)
		{
			matchFound = filter.Filter.MatchAny(toTestKeys, filter.FilterKey);

			if (matchFound)
			{
				// Wait until downloaded.
				Logger.LogInfo($"Obtaining block {filter.Header.BlockHash}...");
				var currentBlock = await _blockProvider(filter.Header.BlockHash, cancellationToken).ConfigureAwait(false);
				if (currentBlock is { })
				{
					_eventBus.Publish(new BlockDownloaded(filter.Header.Height));

					var height = new ChainHeight(filter.Header.Height);
					var blockHash = currentBlock.GetHash();
					var blockTime = currentBlock.Header.BlockTime;
					var blockTransactions = currentBlock.Transactions;
					var txsToProcess = new List<SmartTransaction>(capacity: blockTransactions.Count);

					for (int i = 0; i < blockTransactions.Count; i++)
					{
						var tx = new SmartTransaction(blockTransactions[i], height, blockHash, blockIndex: i, firstSeen: blockTime);
						txsToProcess.Add(tx);
					}

					_transactionProcessor.Process(txsToProcess, isHistoricalReplay: !_initialSynchronizationFinished.Task.IsCompletedSuccessfully || filter.Header.Height <= _replayThroughHeight);
				}
				else
				{
					throw new IOException($"Block {filter.Header.BlockHash} is unavailable; synchronization will retry.");
				}
			}
		}
		return matchFound;
	}

	public void RequestRescan(uint height)
	{
		lock (_rescanGate) { _rescanPending = true; _rescanHeight = _rescanHeight < 0 ? height : Math.Min(_rescanHeight, height); }
	}

	private void QueueReorg(ChainReorganized reorg)
	{
		lock (_reorgTaskGate)
		{
			if (_acceptReorgs)
			{
				var task = ReorgedAsync(reorg.invalidBlockHash, reorg.invalidBlockHeight);
				_pendingReorgs = _pendingReorgs.IsCompleted ? task : Task.WhenAll(_pendingReorgs, task);
			}
		}
	}

	private async Task ReorgedAsync(uint256 invalidBlockHash, ChainHeight invalidBlockHeight)
	{
		try
		{
			var newBestHeight = invalidBlockHeight - 1;

			using (await _reorgLock.LockAsync(CancellationToken.None).ConfigureAwait(false))
			{
				_keyManager.SetMaxBestHeight(newBestHeight);
				_transactionProcessor.UndoBlock(invalidBlockHeight);
				_transactionStore.ReleaseToMempoolFromBlock(invalidBlockHash);
				_blockFilterIterator.RemoveNewerThan(newBestHeight);
			}
		}
		catch (Exception ex)
		{
			Logger.LogWarning(ex);
		}
	}

	public override async Task StartAsync(CancellationToken cancellationToken)
	{
		lock (_reorgTaskGate) { _acceptReorgs = true; }
		_chainReorgSubscription = _eventBus.Subscribe<ChainReorganized>(QueueReorg);
		await base.StartAsync(cancellationToken).ConfigureAwait(false);
	}

	public override void Dispose()
	{
		lock (_reorgTaskGate) { _acceptReorgs = false; }
		_chainReorgSubscription?.Dispose();
		base.Dispose();
	}

	public override async Task StopAsync(CancellationToken cancellationToken)
	{
		Task pendingReorgs;
		lock (_reorgTaskGate)
		{
			_acceptReorgs = false;
			pendingReorgs = _pendingReorgs;
		}
		_chainReorgSubscription?.Dispose();
		try
		{
			await base.StopAsync(cancellationToken).ConfigureAwait(false);
			// BackgroundService can return early when the caller cancels its shutdown wait.
			// Its own stop token still cancels the download and filter loop; await their retirement.
			if (ExecuteTask is { } execution) { await execution.ConfigureAwait(false); }
		}
		finally
		{
			// Callbacks copied before unsubscription may already be waiting on the filter loop.
			await pendingReorgs.ConfigureAwait(false);
		}
	}
}

public static class TaskExtensions
{
	extension(Task)
	{
		public static async Task WaitForAsync(Func<bool> predicate, CancellationToken cancellationToken)
		{
			while (!predicate())
			{
				await Task.Delay(1_000, cancellationToken).ConfigureAwait(false);
			}
		}
	}
}
