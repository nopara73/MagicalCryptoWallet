using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.FeeRateEstimation;
using MagicalCryptoWallet.Tests.UnitTests.Mocks;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.FeeRateEstimation;

public class ExternalFeeRateProviderTests
{
	[Fact]
	public async Task ExternalProviderFailureUsesNextExternalProviderAsync()
	{
		var calls = new List<string>();
		var factory = new MockHttpClientFactory
		{
			OnCreateClient = name => new HttpClient(new ResponseHandler(() =>
				{
					calls.Add(name);
					return name.StartsWith("Blockstream")
						? new HttpResponseMessage(HttpStatusCode.ServiceUnavailable)
						: HttpResponseMessageEx.Ok("""{"fastestFee":8,"halfHourFee":6,"hourFee":4,"economyFee":2}""");
				}))
		};
		var provider = FeeRateProviders.Composed([FeeRateProviders.BlockstreamAsync(factory), FeeRateProviders.MempoolSpaceAsync(factory)]);
		var estimates = await provider(CancellationToken.None);
		Assert.Equal(8m, estimates.GetFeeRate(2).SatoshiPerByte);
		Assert.Equal(["Blockstream-bitcoin-fee-rate-provider", "MempoolSpace-bitcoin-fee-rate-provider"], calls);
	}

	[Fact]
	public async Task UnavailableExternalEstimatesRemainUnavailableAsync()
	{
		var factory = new MockHttpClientFactory
		{
			OnCreateClient = _ => new HttpClient(new ResponseHandler(() => new HttpResponseMessage(HttpStatusCode.ServiceUnavailable)))
		};
		var provider = FeeRateProviders.Composed([FeeRateProviders.BlockstreamAsync(factory), FeeRateProviders.MempoolSpaceAsync(factory)]);
		var estimates = await provider(CancellationToken.None);
		Assert.Empty(estimates.Estimations);
		Assert.False(estimates.TryEstimateConfirmationTime(new FeeRate(2m), out _));
	}

	[Fact]
	public async Task DisabledEstimatesRemainUnavailableAsync()
	{
		var estimates = await FeeRateProviders.NoneAsync()(CancellationToken.None);
		Assert.Empty(estimates.Estimations);
		Assert.False(estimates.TryEstimateConfirmationTime(new FeeRate(2m), out _));
	}

	private sealed class ResponseHandler(Func<HttpResponseMessage> response) : HttpMessageHandler
	{
		protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
		{
			cancellationToken.ThrowIfCancellationRequested();
			return Task.FromResult(response());
		}
	}
}
