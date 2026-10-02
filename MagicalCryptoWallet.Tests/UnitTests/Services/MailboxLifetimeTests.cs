using System;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Services;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Services;

public class MailboxLifetimeTests
{
	[Theory]
	[InlineData(true)]
	[InlineData(false)]
	public async Task AlreadyCancelledRequestDoesNotConstructOrDeliverMessage(bool cancelCaller)
	{
		using var cancelled = new CancellationTokenSource();
		cancelled.Cancel();
		using var worker = new MailboxProcessor<int>("cancelled-request-test", (_, _) => Task.CompletedTask,
			cancellationToken: cancelCaller ? CancellationToken.None : cancelled.Token);
		var constructed = false;
		await Assert.ThrowsAnyAsync<OperationCanceledException>(() => worker.PostAndReplyAsync<int>(reply =>
		{
			constructed = true;
			reply.Reply(1);
			return 1;
		}, cancelCaller ? cancelled.Token : CancellationToken.None));
		Assert.False(constructed);
	}

	[Fact]
	public async Task IdleMailboxCanBeDisposedAndDrained()
	{
		var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
		using var worker = new MailboxProcessor<int>("idle-lifetime-test", async (mailbox, token) =>
		{
			entered.SetResult();
			await mailbox.ReceiveAsync(token);
		});
		worker.Start();
		await entered.Task.WaitAsync(TestContext.Current.CancellationToken);
		worker.Dispose();
		await worker.Completion.WaitAsync(TestContext.Current.CancellationToken);
		Assert.True(worker.Completion.IsCompletedSuccessfully);
	}

	[Fact]
	public async Task CancellationAfterDisposalDoesNotReadTheDisposedTokenSource()
	{
		var entered = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
		var release = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
		using var worker = new MailboxProcessor<int>("lifetime-test", async (_, token) =>
		{
			entered.SetResult();
			await release.Task;
			token.ThrowIfCancellationRequested();
		});
		worker.Start();
		await entered.Task.WaitAsync(TestContext.Current.CancellationToken);
		worker.Dispose();
		release.SetResult();
		await worker.Completion.WaitAsync(TestContext.Current.CancellationToken);
		Assert.True(worker.Completion.IsCompletedSuccessfully);
	}
}
