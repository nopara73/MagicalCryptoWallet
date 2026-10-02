using System;
using System.Linq;
using System.Net.Http;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using NBitcoin.RPC;
using MagicalCryptoWallet.Backend.Models;
using MagicalCryptoWallet.BitcoinRpc;
using MagicalCryptoWallet.Blockchain.Blocks;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Wallets;
using ChainHeight = MagicalCryptoWallet.Models.Height.ChainHeight;
using FilterFetchingResult = MagicalCryptoWallet.Helpers.Result<MagicalCryptoWallet.Services.FiltersResponse, System.TimeSpan>;

namespace MagicalCryptoWallet.TestInfrastructure;

// Only the test projects compile this independent Bitcoin Core oracle.
internal static class RegTestRpcProviders
{
	private const int MaxFiltersPerBitcoinRpcRequest = 100;
	private static readonly FiltersResponse.AlreadyOnBestBlock AlreadyOnBestBlock = new();
	private static readonly FiltersResponse.BestBlockUnknown BestBlockUnknown = new();
	private static FiltersResponse.NewFiltersAvailable NewFiltersAvailable(ChainHeight bestHeight, FilterModel[] filters) => new(bestHeight, filters);

	public static FilterProvider CreateFilterProvider(IRPCClient rpcClient, ConcurrentChain blockHeaderChain) =>
		(fromHeight, fromHash, cancellationToken) => GetFiltersFromBitcoinRpcAsync(rpcClient, blockHeaderChain, fromHash, fromHeight, cancellationToken);

	public static BlockProvider RpcBlockProvider(IRPCClient rpcClient) =>
		async (blockHash, cancellationToken) =>
		{
			try
			{
				return await rpcClient.GetBlockAsync(blockHash, cancellationToken).ConfigureAwait(false);
			}
			catch (Exception ex)
			{
				Logger.LogDebug($"RPC block provider failed to retrieve block {blockHash}: {ex}");
				return null;
			}
		};

	/// <returns>Result with a best blockchain height along with block hashes to retrieve, or a failure indicating a reorg.</returns>
	private static async Task<Result<(ChainHeight BestHeight, uint256[] BlockHashes), bool>> GetBlockHashesAsync(IRPCClient bitcoinRpcClient,
		ConcurrentChain blockHeaderChain, uint256 fromHash, uint fromHeight, CancellationToken cancellationToken)
	{
		if (blockHeaderChain.Tip?.Height > fromHeight)
		{
			var chainBlockHashes = blockHeaderChain
				.EnumerateAfter(fromHash)
				.Select(x => x.HashBlock)
				.Take(MaxFiltersPerBitcoinRpcRequest)
				.ToArray();

			return (BestHeight: (uint)blockHeaderChain.Tip.Height, BlockHashes: chainBlockHashes);
		}

		var currentHeight = await bitcoinRpcClient.GetBlockCountAsync(cancellationToken).ConfigureAwait(false);
		var nbOfFiltersToFetch = Math.Min(MaxFiltersPerBitcoinRpcRequest, currentHeight - (int)fromHeight);

		// = ~ No new block, common case.
		// < ~ Current height can decrease too in a very rare reorg case.
		if (nbOfFiltersToFetch <= 0)
		{
			return nbOfFiltersToFetch < 0
				? Result<(ChainHeight BestHeight, uint256[] BlockHashes), bool>.Fail(true)
				: Result<(ChainHeight BestHeight, uint256[] BlockHashes), bool>.Ok((BestHeight: (uint)currentHeight, BlockHashes: []));
		}

		// Get block hashes from RPC.
		var heights = Enumerable.Range((int)fromHeight, nbOfFiltersToFetch + 1).ToArray();
		var blockHashes = await GetBlockHashesByHeightsFromRpcAsync(bitcoinRpcClient, heights, cancellationToken).ConfigureAwait(false);

		// No block hashes returned by the RPC after RPC reported new blocks. It indicates a reorg.
		if (blockHashes.Length == 0)
		{
			return Result<(ChainHeight BestHeight, uint256[] BlockHashes), bool>.Fail(true);
		}

		// The first block hash returned by the RPC does not match the expected `fromHash`. It indicates a reorg.
		if (blockHashes.Length > 0 && blockHashes[0] != fromHash)
		{
			return Result<(ChainHeight BestHeight, uint256[] BlockHashes), bool>.Fail(true);
		}

		return (BestHeight: (uint)currentHeight, BlockHashes: blockHashes[1..]);
	}

	/// <summary>
	/// Returns the block hashes for the given heights from the Bitcoin RPC. If any of the requests fail, it will return only the successfully completed block hashes.
	/// </summary>
	private static async Task<uint256[]> GetBlockHashesByHeightsFromRpcAsync(IRPCClient bitcoinRpcClient, int[] heights, CancellationToken cancellationToken)
	{
		var batchClient = bitcoinRpcClient.PrepareBatch();
		var blockHashTasks = heights.Select(h => batchClient.GetBlockHashAsync(h, cancellationToken)).ToArray();
		await batchClient.SendBatchAsync(cancellationToken).ConfigureAwait(false);
		// Batch transport completion can precede completion of the individual response tasks.
		// Pending responses are not evidence of a reorg, and must never remove the stored tip.
		try { await Task.WhenAll(blockHashTasks).ConfigureAwait(false); }
		catch (RPCException ex) when (ex.RPCCode == RPCErrorCode.RPC_INVALID_PARAMETER) { /* A genuine reorg can shorten the requested range. */ }

		var blockHashes = blockHashTasks
			.TakeWhile(t => t.IsCompletedSuccessfully)
			.Select(t => t.Result)
			.ToArray();

		return blockHashes;
	}

	/// <summary>
	/// The stored filter tip is orphaned when the block header chain has reached its height
	/// but no longer contains its hash there (the block lost a reorg).
	/// </summary>
	private static bool IsOrphanedFilterTip(ConcurrentChain blockHeaderChain, uint fromHeight, uint256 fromHash)
	{
		if (blockHeaderChain.Tip is not { } headerTip || headerTip.Height < fromHeight)
		{
			// The header chain is behind the stored tip, so it cannot contradict it.
			return false;
		}

		var storedTipBlock = blockHeaderChain.GetBlock(fromHash);
		return storedTipBlock is null || storedTipBlock.Height != (int)fromHeight;
	}

	private static async Task<FilterFetchingResult> GetFiltersFromBitcoinRpcAsync(IRPCClient bitcoinRpcClient, ConcurrentChain blockHeaderChain, uint256 fromHash, uint fromHeight, CancellationToken cancellationToken)
	{
		try
		{
			if (IsOrphanedFilterTip(blockHeaderChain, fromHeight, fromHash))
			{
				// Makes the caller remove the orphaned filter from the store.
				return BestBlockUnknown;
			}

			var result = await GetBlockHashesAsync(bitcoinRpcClient, blockHeaderChain, fromHash, fromHeight, cancellationToken).ConfigureAwait(false);
			if (!result.IsOk)
			{
				return BestBlockUnknown;
			}

			var blockHashes = result.Value.BlockHashes;

			if (blockHashes.Length == 0)
			{
				return AlreadyOnBestBlock;
			}

			var filterBatchClient = bitcoinRpcClient.PrepareBatch();
			var filterTasks = blockHashes.Select(hash => filterBatchClient.GetBlockFilterAsync(hash, cancellationToken))
				.ToArray();
			await filterBatchClient.SendBatchAsync(cancellationToken).ConfigureAwait(false);
			var filterResponses = await Task.WhenAll(filterTasks).ConfigureAwait(false);

			var filters = new FilterModel[blockHashes.Length];
			var height = fromHeight + 1;
			for (var i = 0; i < blockHashes.Length; i++)
			{
				var blockHash = blockHashes[i];
				var filterResponse = filterResponses[i];

				var header = new SmartHeader(blockHash, filterResponse.Header, height, DateTimeOffset.UtcNow);
				var filter = new FilterModel(header, filterResponse.Filter);

				filters[i] = filter;
				height++;
			}

			return NewFiltersAvailable(result.Value.BestHeight, filters);
		}
		catch (RPCException e) when (e.RPCCode == RPCErrorCode.RPC_INVALID_PARAMETER) // Block height out of range
		{
			return BestBlockUnknown;
		}
		catch (Exception e)
		{
			var msg = e is HttpRequestException {InnerException: SocketException}
				? "Cannot connect to get filter from bitcoin RPC"
				: "Error fetching filter from bitcoin RPC";
			Logger.LogError($"{msg} - {e.Message}. Retrying in 15 seconds...");
			return FilterFetchingResult.Fail(TimeSpan.FromSeconds(15));
		}
	}

}
