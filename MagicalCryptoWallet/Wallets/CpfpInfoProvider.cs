using NBitcoin;
using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Data;
using System.Linq;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Mcw.Network;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Serialization;
using MagicalCryptoWallet.Services;

namespace MagicalCryptoWallet.Wallets;

public abstract record CpfpInfoMessage
{
	public record UpdateMessage : CpfpInfoMessage;
	public record GetCachedCpfpInfo(IReplyChannel<CachedCpfpInfo[]> ReplyChannel) : CpfpInfoMessage;
	public record PreFetchInfoForTransaction(SmartTransaction SmartTransaction) : CpfpInfoMessage;
	public record GetInfoForTransaction(SmartTransaction SmartTransaction, IReplyChannel<Result<CpfpInfo, string>> ReplyChannel) : CpfpInfoMessage;
}

public record CachedCpfpInfo(CpfpInfo CpfpInfo, SmartTransaction Transaction);

public class CpfpInfoProvider(MailboxProcessor<CpfpInfoMessage> cpfpUpdater)
{
	public Task<CachedCpfpInfo[]> GetCachedCpfpInfoAsync(CancellationToken cancellationToken) =>
		cpfpUpdater.PostAndReplyAsync<CachedCpfpInfo[]>(chan => new CpfpInfoMessage.GetCachedCpfpInfo(chan), cancellationToken);

	public void ScheduleRequest(SmartTransaction tx) =>
		cpfpUpdater.Post(new CpfpInfoMessage.PreFetchInfoForTransaction(tx));

	public Task<Result<CpfpInfo,string>> GetCpfpInfoAsync(SmartTransaction tx, CancellationToken cancellationToken) =>
		cpfpUpdater.PostAndReplyAsync<Result<CpfpInfo,string>>(chan => new CpfpInfoMessage.GetInfoForTransaction(tx, chan), cancellationToken);
}

public static class CpfpInfoUpdater
{
	public static MessageHandler<CpfpInfoMessage, Unit> CreateForRegTest()
	{
		return (msg, _, _) =>
		{
			// CPFP is not properly supported in regtest yet.
			switch (msg)
			{
				case CpfpInfoMessage.GetCachedCpfpInfo m:
					m.ReplyChannel.Reply([]);
					break;
				case CpfpInfoMessage.GetInfoForTransaction m:
					m.ReplyChannel.Reply(Result<CpfpInfo, string>.Fail("Not implemented for regtest."));
					break;
			}

			return Task.FromResult(Unit.Instance);
		};
	}

	public static MessageHandler<CpfpInfoMessage, Unit> Create(
		IMcwHttpClientFactory httpClientFactory, Network network, EventBus eventBus, TimeProvider? timeProvider = null,
		Func<TimeSpan, CancellationToken, Task>? prefetchDelay = null)
	{
		var uri = network == Network.Main
			? new Uri("https://mempool.space/api/")
			: new Uri("https://mempool.space/testnet4/api/");
		var tasks = new Dictionary<uint256, Task>();
		var cache = new ConcurrentDictionary<uint256, CachedCpfpInfo>();
		var pending = new ConcurrentDictionary<uint256, Lazy<Task<Result<CpfpInfo, string>>>>();
		var failures = new ConcurrentDictionary<uint256, (DateTimeOffset RetryAt, string Error)>();
		return (msg, _, cancellationToken) => ProcessMessagesAsync(msg, httpClientFactory, uri, tasks, cache, pending, failures,
			timeProvider ?? TimeProvider.System, prefetchDelay ?? Task.Delay, eventBus, cancellationToken);
	}

	private static async Task<Unit> ProcessMessagesAsync(CpfpInfoMessage msg, IMcwHttpClientFactory httpClientFactory, Uri uri,
		Dictionary<uint256, Task> tasks, ConcurrentDictionary<uint256, CachedCpfpInfo> cache,
		ConcurrentDictionary<uint256, Lazy<Task<Result<CpfpInfo, string>>>> pending,
		ConcurrentDictionary<uint256, (DateTimeOffset RetryAt, string Error)> failures, TimeProvider timeProvider,
		Func<TimeSpan, CancellationToken, Task> prefetchDelay, EventBus eventBus, CancellationToken cancellationToken)
	{
		switch (msg)
		{
			case CpfpInfoMessage.UpdateMessage _ :
				await ProcessFinishedFetchingTasksAsync(tasks, cancellationToken).ConfigureAwait(false);
				CleanCache(cache);
				foreach (var expired in failures.Where(x => x.Value.RetryAt <= timeProvider.GetUtcNow())) { failures.TryRemove(expired.Key, out _); }
				break;
			case CpfpInfoMessage.GetCachedCpfpInfo m:
				m.ReplyChannel.Reply(cache.Values.ToArray());
				break;
			case CpfpInfoMessage.GetInfoForTransaction m:
				var cpfpInfo = await GetCpfpInfo(m.SmartTransaction).ConfigureAwait(false);
				m.ReplyChannel.Reply(cpfpInfo);
				break;
			case CpfpInfoMessage.PreFetchInfoForTransaction m:
				var txid = m.SmartTransaction.GetHash();
				if (!cache.ContainsKey(txid) && (!failures.TryGetValue(txid, out var failure) || failure.RetryAt <= timeProvider.GetUtcNow()) &&
					(!tasks.TryGetValue(txid, out var task) || task.IsCompleted))
				{
					tasks[txid] = ScheduleTaskAsync(m.SmartTransaction, GetCpfpInfo, prefetchDelay, cancellationToken);
				}
				break;
		}

		return Unit.Instance;

		async Task<Result<CpfpInfo, string>> GetCpfpInfo(SmartTransaction tx)
		{
			var result = await GetCpfpInfoAsync(tx, httpClientFactory, uri, cache, pending, failures, timeProvider, cancellationToken).ConfigureAwait(false);
			return result.Map(
				info =>
				{
					eventBus.Publish(new CpfpInfoArrived());
					return info;
				});
		}
	}

	private static async Task ProcessFinishedFetchingTasksAsync(Dictionary<uint256, Task> tasks, CancellationToken cancellationToken)
	{
		cancellationToken.ThrowIfCancellationRequested();
		var completedTasks = tasks.Where(t => t.Value.IsCompleted).ToArray();
		await Task.WhenAll(completedTasks.Select(t => t.Value)).ConfigureAwait(false);
		foreach (var task in completedTasks) { tasks.Remove(task.Key); }
	}

	private static void CleanCache(ConcurrentDictionary<uint256, CachedCpfpInfo> cache)
	{
		var confirmed = cache.Where(e => e.Value.Transaction.Confirmed).ToArray();

		foreach (var cacheEntry in confirmed)
		{
			cache.TryRemove(cacheEntry.Key, out _);
		}
	}

	internal static async Task ScheduleTaskAsync(SmartTransaction transaction, Func<SmartTransaction, Task<Result<CpfpInfo, string>>> cpfpGetter,
		Func<TimeSpan, CancellationToken, Task> prefetchDelay, CancellationToken cancellationToken)
	{
		if (!transaction.CanBeSpeedUpUsingCpfp())
		{
			return;
		}

		const int MaximumDelayInMilliseconds = 10_000;
		var random = RandomnessProviders.Secure;
		var delayInMilliseconds = random.GetInt(MaximumDelayInMilliseconds);
		var delay = TimeSpan.FromMilliseconds(delayInMilliseconds);

		try
		{
			await prefetchDelay(delay, cancellationToken).ConfigureAwait(false);
			cancellationToken.ThrowIfCancellationRequested();
			if (!transaction.CanBeSpeedUpUsingCpfp()) { return; }
			await cpfpGetter(transaction).ConfigureAwait(false);
		}
		catch (OperationCanceledException)
		{
			if (cancellationToken.IsCancellationRequested)
			{
				Logger.LogTrace($"FetchCpfpInfoAsync was canceled for {transaction.GetHash()} because Magical Crypto Wallet is shutting down");
			}
		}
	}

	private static async Task<Result<CpfpInfo, string>> GetCpfpInfoAsync(SmartTransaction tx, IMcwHttpClientFactory httpClientFactory, Uri uri,
		ConcurrentDictionary<uint256, CachedCpfpInfo> cache, ConcurrentDictionary<uint256, Lazy<Task<Result<CpfpInfo, string>>>> pending,
		ConcurrentDictionary<uint256, (DateTimeOffset RetryAt, string Error)> failures, TimeProvider timeProvider, CancellationToken cancellationToken)
	{
		var txid = tx.GetHash();
		if (cache.TryGetValue(txid, out var cachedCpfpInfo))
		{
			return cachedCpfpInfo.CpfpInfo;
		}
		if (failures.TryGetValue(txid, out var failure) && failure.RetryAt > timeProvider.GetUtcNow())
		{
			return Result<CpfpInfo, string>.Fail(failure.Error);
		}

		var request = pending.GetOrAdd(txid, _ => new Lazy<Task<Result<CpfpInfo, string>>>(FetchAsync));
		try { return await request.Value.ConfigureAwait(false); }
		finally { pending.TryRemove(new KeyValuePair<uint256, Lazy<Task<Result<CpfpInfo, string>>>>(txid, request)); }

		async Task<Result<CpfpInfo, string>> FetchAsync()
		{
			if (cache.TryGetValue(txid, out var completed)) { return completed.CpfpInfo; }
			try
			{
				var cpfpInfo = await GetCpfpInfoAsync(txid, httpClientFactory, uri, cancellationToken).ConfigureAwait(false);
				cache.TryAdd(txid, new CachedCpfpInfo(cpfpInfo, tx));
				failures.TryRemove(txid, out _);
				return cpfpInfo;
			}
			catch (OperationCanceledException e) when (cancellationToken.IsCancellationRequested)
			{
				return Result<CpfpInfo, string>.Fail(e.Message);
			}
			catch (Exception e)
			{
				failures[txid] = (timeProvider.GetUtcNow() + TimeSpan.FromSeconds(30), e.Message);
				return Result<CpfpInfo, string>.Fail(e.Message);
			}
		}
	}

	private static async Task<CpfpInfo> GetCpfpInfoAsync(uint256 txid, IMcwHttpClientFactory httpClientFactory, Uri uri, CancellationToken cancellationToken)
	{
		using var timeoutCts = new CancellationTokenSource(TimeSpan.FromSeconds(20));
		using var linkedCts = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, timeoutCts.Token);

		using var httpClient = httpClientFactory.CreateClient($"mempool.space-{txid}");
		httpClient.BaseAddress = uri;
		using var request = new HttpRequestMessage(HttpMethod.Get, $"v1/cpfp/{txid}");
		using var response = await httpClient.SendAsync(request, linkedCts.Token).ConfigureAwait(false);

		response.EnsureSuccessStatusCode();

		var stringResponse = await response.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);

		return JsonDecoder.FromString(stringResponse, Decode.CpfpInfo)
			?? throw new DataException("Deserialization error");
	}
}
