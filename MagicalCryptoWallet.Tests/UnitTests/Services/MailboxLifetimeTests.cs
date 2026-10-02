using System.Threading.Tasks;
using MagicalCryptoWallet.Services;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Services;

public class MailboxLifetimeTests
{
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
