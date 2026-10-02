using MagicalCryptoWallet.Backend.Models;
using MagicalCryptoWallet.BitcoinP2p;
using MagicalCryptoWallet.Blockchain.Blocks;
using MagicalCryptoWallet.Stores;

namespace MagicalCryptoWallet.Services;


using FilterFetchingResult = Result<FiltersResponse, TimeSpan>;

public abstract record FiltersResponse
{
	public record AlreadyOnBestBlock : FiltersResponse;
	public record BestBlockUnknown : FiltersResponse;
	public record NewFiltersAvailable(ChainHeight BestHeight, FilterModel[] Filters) : FiltersResponse;
}

public delegate Task<FilterFetchingResult> FilterProvider(uint fromHeight, uint256 fromHash, CancellationToken cancellationToken);

public static class FilterProviders
{
	public static readonly TimeSpan WaitForBlockHeadersToCatchUp = TimeSpan.FromSeconds(15);

	private static readonly FiltersResponse.AlreadyOnBestBlock AlreadyOnBestBlock = new();
	private static readonly FiltersResponse.BestBlockUnknown BestBlockUnknown = new();
	private static FiltersResponse.NewFiltersAvailable NewFiltersAvailable(ChainHeight bestHeight, FilterModel[] filters) => new(bestHeight, filters);

	public static FilterProvider CreateBitcoinP2pFilterProvider(FilterHeaderChain filterHeadersChain, ConcurrentChain blockHeadersChain, FilterSynchronizationState synchronizationState) =>
		(fromHeight, fromHash, cancellationToken) => GetFiltersFromBitcoinP2pAsync(filterHeadersChain, blockHeadersChain, synchronizationState, fromHeight, fromHash, cancellationToken);

	private static async Task<FilterFetchingResult> GetFiltersFromBitcoinP2pAsync(
		FilterHeaderChain filterHeadersChain,
		ConcurrentChain blockHeadersChain,
		FilterSynchronizationState synchronizationState,
		uint fromHeight,
		uint256 fromHash,
		CancellationToken cancellationToken)
	{
		try
		{
			var filterHeadersTip = filterHeadersChain.Tip;
			if (filterHeadersTip is null)
			{
				Logger.LogTrace("Filter headers tip is null. Retrying in 1 second.");
				return FilterFetchingResult.Fail(TimeSpan.FromSeconds(1));
			}

			// Block headers are synchronized from scratch. Filter headers are synchronized from an appropriate checkpoint.
			if (blockHeadersChain.Height < filterHeadersChain.TipHeight)
			{
				Logger.LogTrace($"Block headers chain is not synchronized yet ({blockHeadersChain.Height} < {filterHeadersChain.TipHeight}). Retrying in {WaitForBlockHeadersToCatchUp.TotalSeconds} seconds.");
				return FilterFetchingResult.Fail(WaitForBlockHeadersToCatchUp);
			}

			// Must run before the height comparison below: after a tip reorg the filter header
			// tip sits at the same height as the stored tip, just on another block.
			if (synchronizationState.IsReorg(fromHeight, fromHash))
			{
				return BestBlockUnknown;
			}

			if (filterHeadersTip.Height < fromHeight)
			{
				Logger.LogTrace($"Filter headers not synced past current position (tip: {filterHeadersTip.Height}, current: {fromHeight}), retrying in 1 second");
				return FilterFetchingResult.Fail(TimeSpan.FromSeconds(1));
			}

			if (filterHeadersTip.Height == fromHeight)
			{
				return AlreadyOnBestBlock;
			}

			Logger.LogDebug($"Requesting filters from height {fromHeight + 1} (filter headers tip: {filterHeadersTip.Height})");

			// Consume filters from the async stream (one page at a time)
			using var timeoutCts = new CancellationTokenSource(TimeSpan.FromSeconds(90));
			using var linkedCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, timeoutCts.Token);

			try
			{
				var filters = await synchronizationState.GetNextFilterBatchAsync(linkedCts.Token).ConfigureAwait(false);

				if (filters.Length == 0)
				{
					Logger.LogWarning("Received 0 filters from P2P. Retrying in 1 second");
					return FilterFetchingResult.Fail(TimeSpan.FromSeconds(1));
				}

				Logger.LogDebug($"Successfully received {filters.Length} filters from P2P (heights {filters[0].Header.Height}-{filters[^1].Header.Height})");
				return NewFiltersAvailable((uint)blockHeadersChain.Tip.Height, filters.ToArray());
			}
			catch (OperationCanceledException) when (timeoutCts.IsCancellationRequested)
			{
				// Timeout - retry
				Logger.LogWarning($"Timeout (90s) waiting for filters from P2P at height {fromHeight + 1}. Retrying in 1 second...");
				return FilterFetchingResult.Fail(TimeSpan.FromSeconds(1));
			}
		}
		catch (OperationCanceledException)
		{
			throw;
		}
		catch (Exception e)
		{
			Logger.LogError($"Error waiting for filters from P2P: {e}. Retrying in 15 seconds...");
			return FilterFetchingResult.Fail(TimeSpan.FromSeconds(15));
		}
	}
}

public static class Synchronizer
{
	public static MessageHandler<Unit> CreateFilterGenerator(FilterProvider filtersProvider, FilterStore filterStore, FilterHeaderChain filterHeaderChain, EventBus eventBus) =>
		(_, cancellationToken) => GenerateCompactFiltersAsync(filtersProvider, filterStore, filterHeaderChain, eventBus, cancellationToken);

	private static async Task<Unit> GenerateCompactFiltersAsync(FilterProvider filtersProvider, FilterStore filterStore, FilterHeaderChain filterHeaderChain, EventBus eventBus, CancellationToken cancellationToken)
	{
		// Don't attempt synchronization without a valid tip hash
		if (filterHeaderChain.TipHash is null)
		{
			await Task.Delay(TimeSpan.FromSeconds(0.5), cancellationToken).ConfigureAwait(false);
			return Unit.Instance;
		}

		if (filterStore.GetTip() is not { } storedTip)
		{
			return Unit.Instance;
		}

		var response = await filtersProvider(storedTip.Header.Height, storedTip.Header.BlockHash, cancellationToken)
			.ConfigureAwait(false);

		if (response.IsOk)
		{
			var isSynchronized = await ProcessFiltersAsync(response.Value, filterStore, filterHeaderChain, eventBus).ConfigureAwait(false);
			if (isSynchronized)
			{
				await Task.Delay(TimeSpan.FromSeconds(20), cancellationToken).ConfigureAwait(false);
			}
		}
		else
		{
			var continueAfterSeconds = response.Error;
			await Task.Delay(continueAfterSeconds, cancellationToken).ConfigureAwait(false);
		}
		return Unit.Instance;
	}

	private static async Task<bool> ProcessFiltersAsync(FiltersResponse response, FilterStore filterStore, FilterHeaderChain filterHeaderChain, EventBus eventBus)
	{
		switch (response)
		{
			case FiltersResponse.AlreadyOnBestBlock:
				// Already synchronized. Nothing to do.
				var tip = filterHeaderChain.TipHeight;
				filterHeaderChain.SetServerTipHeight(tip);
				eventBus.Publish(new NetworkTipHeightChanged(tip));
				return true;
			case FiltersResponse.BestBlockUnknown:
				// Reorg happened. Rollback the latest index.
				FilterModel reorgedFilter = await filterStore.TryRemoveLastFilterAsync().ConfigureAwait(false)
					?? throw new InvalidOperationException("Fatal error: Failed to remove the reorged filter.");

				Logger.LogInfo($"REORG Invalid Block: {reorgedFilter.Header.BlockHash}  Height {reorgedFilter.Header.Height}.");
				break;
			case FiltersResponse.NewFiltersAvailable newFiltersAvailable:
				var localTipHeight = filterStore.GetTip()?.Header.Height ?? 0;

				filterHeaderChain.SetServerTipHeight(newFiltersAvailable.BestHeight);
				eventBus.Publish(new NetworkTipHeightChanged(newFiltersAvailable.BestHeight));

				var downloadedFilters = newFiltersAvailable.Filters;
				var newFilters = downloadedFilters.Where(x => localTipHeight < x.Header.Height).ToArray();
				var firstNewFilter = newFilters.FirstOrDefault();

				if (firstNewFilter is null)
				{
					Logger.LogInfo(downloadedFilters.Length == 1
						? $"Downloaded filter for block {downloadedFilters[0].Header.Height} is known locally."
						: $"Downloaded filters for blocks from {downloadedFilters[0].Header.Height} to {downloadedFilters[^1].Header.Height} are known locally.");
				}
				else if (localTipHeight + 1 != firstNewFilter.Header.Height)
				{
					// We have a problem.
					// We have wrong filters, the heights are not in sync with the server's.
					string details = FormatInconsistencyDetails(filterHeaderChain, firstNewFilter);
					Logger.LogError($"Inconsistent index state detected.{Environment.NewLine}{details}");

					await filterStore.RemoveAllNewerThanAsync(localTipHeight).ConfigureAwait(false);
				}
				else
				{
					await filterStore.AddNewFiltersAsync(newFilters).ConfigureAwait(false);

					Logger.LogInfo(newFilters.Length == 1
						? $"Downloaded filter for block {firstNewFilter.Header.Height}."
						: $"Downloaded filters for blocks from {firstNewFilter.Header.Height} to {newFilters.Last().Header.Height}.");
				}

				break;
			default:
				throw new ArgumentOutOfRangeException(nameof(response));
		}

		return false;
	}

	private static string FormatInconsistencyDetails(FilterHeaderChain hashChain, FilterModel firstFilter)
	{
		return string.Join(
			Environment.NewLine,
			[
				$"  Local Chain:",
				$"    Tip Height: {hashChain.TipHeight}",
				$"    Tip Hash: {hashChain.TipHash}",
				$"    Hashes Left: {hashChain.HashesLeft}",
				$"    Hash Count: {hashChain.HashCount}",
				$"  Server:",
				$"    Server Tip Height: {hashChain.ServerTipHeight}",
				$"  First Filter:",
				$"    Block Hash: {firstFilter.Header.BlockHash}",
				$"    Height: {firstFilter.Header.Height}"
			]);
	}
}
