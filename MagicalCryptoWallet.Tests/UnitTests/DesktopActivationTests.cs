using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Tests.Helpers;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class DesktopActivationTests
{
	[Fact]
	public async Task ActivationQueuesUntilBoundAndSilentDuplicatesNeverShowAWindowAsync()
	{
		var root = await Common.GetEmptyWorkDirAsync();
		await using var owner = new DesktopActivation(root, Network.RegTest);
		int shown = 0;
		Assert.True(await DesktopActivation.RequestAsync(root, Network.RegTest, silent: true));
		Assert.True(await DesktopActivation.RequestAsync(root, Network.RegTest, silent: false));
		owner.Bind(() => Interlocked.Increment(ref shown));
		Assert.Equal(1, shown);
		Assert.True(await DesktopActivation.RequestAsync(root, Network.RegTest, silent: true));
		Assert.Equal(1, shown);
		Assert.False(await DesktopActivation.RequestAsync(root, Network.Main, silent: false));
		Assert.Equal(1, shown);
		Assert.True(await DesktopActivation.RequestAsync(root, Network.RegTest, silent: false));
		Assert.Equal(2, shown);
	}
	[Fact]
	public async Task DaemonLockCannotBeActivatedAndDisposalReleasesDirectoryAsync()
	{
		var root = await Common.GetEmptyWorkDirAsync();
		using var daemon = new SingleInstanceChecker(root);
		using var duplicate = new SingleInstanceChecker(root);
		Assert.True(daemon.IsFirstInstance());
		Assert.False(duplicate.IsFirstInstance());
		Assert.False(await DesktopActivation.RequestAsync(root, Network.RegTest, silent: false));
		daemon.Dispose();
		Assert.True(duplicate.IsFirstInstance());
	}
}
