using System.Diagnostics;
using System.Collections.Generic;
using System.Net;
using System.Net.Http;
using System.Net.Http.Headers;
using System.Net.Mime;
using System.Text;
using System.Threading.Tasks;
using System.Threading;
using MagicalCryptoWallet.Tests.UnitTests.Mocks;
using MagicalCryptoWallet.WebClients.MagicalCryptoWallet;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.WebClients.MagicalCryptoWallet;

public class RetryHttpClientHandlerTests
{
	[Theory]
	[InlineData(false)]
	[InlineData(true)]
	public async Task RetryAfterIsRespectedAndFailedResponseIsDisposedBeforeWaitingAsync(bool absolute)
	{
		var clock = new ManualTimeProvider();
		using var content = new TrackingContent();
		var calls = 0;
		var delays = new List<TimeSpan>();
		using var handler = new RetryHttpClientHandler("retry-after", _ => { }, HttpClientHandlerConfiguration.Default,
			(_, _, _) =>
			{
				var response = new HttpResponseMessage(++calls == 1 ? HttpStatusCode.TooManyRequests : HttpStatusCode.OK);
				if (calls == 1)
				{
					response.Content = content;
					response.Headers.RetryAfter = absolute
						? new RetryConditionHeaderValue(clock.GetUtcNow().AddSeconds(9))
						: new RetryConditionHeaderValue(TimeSpan.FromSeconds(9));
				}
				return Task.FromResult(response);
			}, (delay, _) =>
			{
				Assert.True(content.IsDisposed);
				delays.Add(delay);
				return Task.CompletedTask;
			}, clock);
		using var client = new HttpClient(handler);
		using var response = await client.GetAsync("http://synthetic.invalid/");
		Assert.Equal(HttpStatusCode.OK, response.StatusCode);
		Assert.Equal(2, calls);
		Assert.Equal(TimeSpan.FromSeconds(9), Assert.Single(delays));
	}

	[Fact]
	public async Task RetryBudgetBacksOffWithoutSleepingAfterFinalAttemptAsync()
	{
		var calls = 0;
		var delays = new List<TimeSpan>();
		var contents = new List<TrackingContent>();
		using var handler = new RetryHttpClientHandler("budget", _ => { }, HttpClientHandlerConfiguration.Default,
			(_, _, _) =>
			{
				calls++;
				var content = new TrackingContent();
				contents.Add(content);
				return Task.FromResult(new HttpResponseMessage(HttpStatusCode.ServiceUnavailable) { Content = content });
			}, (delay, _) => { delays.Add(delay); return Task.CompletedTask; });
		using var client = new HttpClient(handler);
		await Assert.ThrowsAsync<HttpRequestException>(() => client.GetAsync("http://synthetic.invalid/"));
		Assert.Equal(3, calls);
		Assert.Equal(new[] { TimeSpan.FromSeconds(2), TimeSpan.FromSeconds(4) }, delays);
		Assert.All(contents, c => Assert.True(c.IsDisposed));
	}

	[Fact]
	public async Task CancellationDuringServerRequestedWaitStopsRetriesAsync()
	{
		var waiting = new TaskCompletionSource(TaskCreationOptions.RunContinuationsAsynchronously);
		var calls = 0;
		using var cancellation = new CancellationTokenSource(TimeSpan.FromSeconds(10));
		using var handler = new RetryHttpClientHandler("cancel", _ => { }, HttpClientHandlerConfiguration.Default,
			(_, _, _) =>
			{
				calls++;
				var response = new HttpResponseMessage(HttpStatusCode.TooManyRequests);
				response.Headers.RetryAfter = new RetryConditionHeaderValue(TimeSpan.FromHours(1));
				return Task.FromResult(response);
			}, async (delay, token) =>
			{
				Assert.Equal(TimeSpan.FromHours(1), delay);
				waiting.TrySetResult();
				await Task.Delay(Timeout.InfiniteTimeSpan, token);
			});
		using var client = new HttpClient(handler);
		var request = client.GetAsync("http://synthetic.invalid/", cancellation.Token);
		await waiting.Task.WaitAsync(cancellation.Token);
		await cancellation.CancelAsync();
		await Assert.ThrowsAnyAsync<OperationCanceledException>(() => request);
		Assert.Equal(1, calls);
	}

	private sealed class TrackingContent() : ByteArrayContent([])
	{
		public bool IsDisposed { get; private set; }
		protected override void Dispose(bool disposing)
		{
			IsDisposed = true;
			base.Dispose(disposing);
		}
	}

	// Trivial test to make sure that the mock handler works as expected.
	[Fact]
	public async Task SendAsync_OkTestAsync()
	{
		var callbackCalled = false;

		var handler = new RetryHttpClientHandler("retry-handler", _ => callbackCalled = true,
			HttpClientHandlerConfiguration.Default,
			(_, _, _) =>
			{
				var responseMessage = new HttpResponseMessage(HttpStatusCode.OK);
				responseMessage.Content =
					new StringContent("My Response", Encoding.UTF8, MediaTypeNames.Text.Plain);
				return Task.FromResult(responseMessage);
			});

		using (handler)
		{
			using var httpClient = new HttpClient(handler, disposeHandler: false);
			using var request = new HttpRequestMessage(HttpMethod.Get, "http://test.dev");

			using var response = await httpClient.SendAsync(request);
			Assert.Equal(HttpStatusCode.OK, response.StatusCode);

			using var stringContent = Assert.IsType<StringContent>(response.Content);
			var payload = await stringContent.ReadAsStringAsync();
			Assert.Equal("My Response", payload);
		}

		Assert.True(callbackCalled);
	}

	// Tests that the handler stops retrying if .
	[Fact]
	public async Task SendAsync_RepeatingStopsAsync()
	{
		var callbackCalled = false;
		var requestsCount = 0;
		var handler = new RetryHttpClientHandler("retry-handler", _ => callbackCalled = true, HttpClientHandlerConfiguration.Default,
			(retryHandler, _, _) =>
			{
				requestsCount++;

				if (requestsCount == 1)
				{
					// Simulate a disposed handler on the first request.
					retryHandler.Dispose();

					// Force the HTTP handler to repeat the request.
					throw new HttpRequestException(HttpRequestError.ConnectionError, "Make sure the request is repeated.");
				}
				throw new UnreachableException();
			});

		using (handler)
		{
			using var httpClient = new HttpClient(handler, disposeHandler: false);
			using var request = new HttpRequestMessage(HttpMethod.Get, "http://test.dev");

			// Expect an ObjectDisposedException since the handler was disposed during the first request.
			var e = await Assert.ThrowsAsync<TimeoutException>(async () => await httpClient.SendAsync(request).ConfigureAwait(false));
			Assert.Equal("HTTP handler 'retry-handler' was disposed during request.", e.Message);

			Assert.Equal(1, requestsCount);
		}
		Assert.True(callbackCalled);
	}

	// Tests that an ObjectDisposedException is thrown when the handler is disposed before sending an HTTP request.
	[Fact]
	public async Task SendAsync_DisposeExceptionIsThrownAsync()
	{
		var callbackCalled = false;

		var handler = new RetryHttpClientHandler("retry-handler", _ => callbackCalled = true, HttpClientHandlerConfiguration.Default,
			(_, _, _) =>
			{
				var responseMessage = new HttpResponseMessage(HttpStatusCode.OK);
				responseMessage.Content = new StringContent("My Response", Encoding.UTF8, MediaTypeNames.Text.Plain);
				return Task.FromResult(responseMessage);
			});

		using var httpClient = new HttpClient(handler, disposeHandler: false);
		// Dispose the handler to trigger the disposed exception.
		handler.Dispose();

		// Send a request after disposing the handler.
		using var request = new HttpRequestMessage(HttpMethod.Get, "http://test.dev");

		// Expect an ObjectDisposedException since the handler was disposed before sending the request.
		await Assert.ThrowsAsync<ObjectDisposedException>(async () => await httpClient.SendAsync(request).ConfigureAwait(false));

		Assert.True(callbackCalled);
	}
}
