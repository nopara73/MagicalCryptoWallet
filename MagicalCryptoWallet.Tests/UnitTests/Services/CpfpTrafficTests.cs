using System.Linq;
using System.Net;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Tests.UnitTests.Mocks;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Services;

public class CpfpTrafficTests
{
	private const string CpfpJson = """{"effectiveFeePerVsize": 10.5, "fee": 1.0, "adjustedVsize": 100, "ancestors": []}""";

	[Fact]
	public async Task OverlappingRequestsForSameTransactionShareOneHttpRequestAsync()
	{
		using var cancellation = new CancellationTokenSource(TimeSpan.FromSeconds(15));
		var started = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
		var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
		var requests = 0;
		var factory = new MockHttpClientFactory
		{
			OnCreateClient = _ => new MockHttpClient
			{
				OnSendAsync = async request =>
				{
					Interlocked.Increment(ref requests);
					Assert.StartsWith("v1/cpfp/", request.RequestUri!.OriginalString);
					started.TrySetResult();
					await release.Task.WaitAsync(cancellation.Token);
					return HttpResponseMessageEx.Ok(CpfpJson);
				}
			}
		};
		var handler = CpfpInfoUpdater.Create(factory, Network.Main, new EventBus());
		var tx = BitcoinFactory.CreateSmartTransaction(height: Height.Mempool);
		var replies = Enumerable.Range(0, 8).Select(_ => new TestReplyChannel<Result<CpfpInfo, string>>()).ToArray();
		var tasks = replies.Select(reply => handler(new CpfpInfoMessage.GetInfoForTransaction(tx, reply), Unit.Instance, cancellation.Token)).ToArray();
		await started.Task.WaitAsync(cancellation.Token);
		Assert.Equal(1, requests);
		release.TrySetResult();
		await Task.WhenAll(tasks);
		Assert.All(replies, reply => Assert.True(reply.Result!.IsOk));
		Assert.Equal(1, requests);
	}

	[Fact]
	public async Task FailedRequestsWaitForCooldownAndUpdatesDoNotRefetchCachedInfoAsync()
	{
		var requests = 0;
		var factory = new MockHttpClientFactory
		{
			OnCreateClient = _ => new MockHttpClient
			{
				OnSendAsync = _ => Task.FromResult(Interlocked.Increment(ref requests) == 1
					? new System.Net.Http.HttpResponseMessage(HttpStatusCode.InternalServerError)
					: HttpResponseMessageEx.Ok(CpfpJson))
			}
		};
		var clock = new ManualTimeProvider();
		var handler = CpfpInfoUpdater.Create(factory, Network.Main, new EventBus(), clock);
		var tx = BitcoinFactory.CreateSmartTransaction(height: Height.Mempool);
		var failed = new TestReplyChannel<Result<CpfpInfo, string>>();
		await handler(new CpfpInfoMessage.GetInfoForTransaction(tx, failed), Unit.Instance, CancellationToken.None);
		Assert.False(failed.Result!.IsOk);
		var success = new TestReplyChannel<Result<CpfpInfo, string>>();
		await handler(new CpfpInfoMessage.GetInfoForTransaction(tx, success), Unit.Instance, CancellationToken.None);
		Assert.False(success.Result!.IsOk);
		Assert.Equal(1, requests);
		clock.Advance(TimeSpan.FromSeconds(30));
		await handler(new CpfpInfoMessage.GetInfoForTransaction(tx, success), Unit.Instance, CancellationToken.None);
		Assert.True(success.Result!.IsOk);
		for (var n = 0; n < 5; n++) { await handler(new CpfpInfoMessage.UpdateMessage(), Unit.Instance, CancellationToken.None); }
		await handler(new CpfpInfoMessage.GetInfoForTransaction(tx, success), Unit.Instance, CancellationToken.None);
		Assert.Equal(2, requests);
	}

	[Theory]
	[InlineData(true)]
	[InlineData(false)]
	public async Task PrefetchRechecksConfirmationAfterDelayAsync(bool confirmed)
	{
		var waiting = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
		var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
		var requests = 0;
		var tx = BitcoinFactory.CreateSmartTransaction(ownOutputCount: 1, height: Height.Mempool);
		Assert.True(tx.CanBeSpeedUpUsingCpfp());
		using var cancellation = new CancellationTokenSource(TimeSpan.FromSeconds(10));
		var work = CpfpInfoUpdater.ScheduleTaskAsync(tx, _ =>
		{
			requests++;
			return Task.FromResult(Result<CpfpInfo, string>.Fail("synthetic"));
		}, async (_, token) =>
		{
			waiting.TrySetResult();
			await release.Task.WaitAsync(token);
		}, cancellation.Token);
		await waiting.Task.WaitAsync(cancellation.Token);
		if (confirmed) { tx.TryUpdate(new SmartTransaction(tx.Transaction, new Height.ChainHeight(1))); }
		release.TrySetResult();
		await work;
		Assert.Equal(confirmed ? 0 : 1, requests);
	}
}
