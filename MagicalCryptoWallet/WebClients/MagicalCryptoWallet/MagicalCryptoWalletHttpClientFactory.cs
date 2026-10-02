using System.Collections.Concurrent;
using System.Linq;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Mcw.Network;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.WebClients.MagicalCryptoWallet;

public delegate DateTime LifetimeResolver(string identity);

public record HttpClientHandlerConfiguration
{
	public static readonly HttpClientHandlerConfiguration Default = new();
	public int MaxAttempts { get; init; } = 3;
	public TimeSpan TimeBeforeRetryingAfterTooManyRequests { get; init; } = TimeSpan.FromSeconds(2);
	public TimeSpan TimeBeforeRetryingAfterNetworkError { get; init; } = TimeSpan.FromSeconds(3);
	public TimeSpan TimeBeforeRetryingAfterServerError { get; init; } = TimeSpan.FromSeconds(2);
}

public class HttpClientFactory : IMcwHttpClientFactory
{
	private readonly HttpClientHandlerConfiguration _httpHandlerConfig;
	private readonly ConcurrentDictionary<string, DateTime> _expirationDatetimes = new();
	private readonly ConcurrentDictionary<string, HttpClientHandler> _httpClientHandlers = new();
	private readonly ConcurrentBag<LifetimeResolver> _lifetimeResolvers = new();

	public HttpClientFactory(HttpClientHandlerConfiguration? httpHandlerConfig = null)
	{
		_httpHandlerConfig = httpHandlerConfig ?? HttpClientHandlerConfiguration.Default;
		AddLifetimeResolver(identity => identity.StartsWith("long-live")
			? DateTime.MaxValue
			: DateTime.UtcNow.AddHours(6));
	}

	public HttpClient CreateClient(string name)
	{
		CheckForExpirations();
		var httpClientHandler = _httpClientHandlers.GetOrAdd(name, CreateHttpClientHandler);
		return new HttpClient(httpClientHandler, false);
	}

	public void AddLifetimeResolver(LifetimeResolver resolver)
	{
		_lifetimeResolvers.Add(resolver);
	}

	private void CheckForExpirations()
	{
		var expiredHandlers = _expirationDatetimes.Where(x => x.Value < DateTime.UtcNow).Select(x => x.Key).ToArray();
		foreach (var handlerName in expiredHandlers)
		{
			if (_httpClientHandlers.TryRemove(handlerName, out var handler))
			{
				handler.Dispose();
			}
		}
	}

	protected virtual HttpClientHandler CreateHttpClientHandler(string name)
	{
		Logger.LogDebug($"Create HTTP handler {name}");
		SetExpirationDate(name);
		var handler = new RetryHttpClientHandler(name,
			handlerName =>
			{
				_httpClientHandlers.TryRemove(handlerName, out _);
				_expirationDatetimes.TryRemove(handlerName, out _);
			}, _httpHandlerConfig);

		handler.AutomaticDecompression = DecompressionMethods.All;
		return handler;
	}

	private void SetExpirationDate(string name)
	{
		var expirationTime = _lifetimeResolvers.Min(resolve => resolve(name));
		_expirationDatetimes.AddOrUpdate(name, expirationTime, (_, _) => expirationTime);
	}
}

public class OnionHttpClientFactory(Uri proxyUri, HttpClientHandlerConfiguration? configurator = null)
	: HttpClientFactory(configurator)
{
	protected override HttpClientHandler CreateHttpClientHandler(string name)
	{
		var credentials = new NetworkCredential(name, name);
		var webProxy = new LoopbackBypassProxy(proxyUri, credentials);
		var handler = base.CreateHttpClientHandler(name);
		handler.Proxy = webProxy;
		return handler;
	}
}

/// <summary>Public requests connect directly, independently of the wallet's Tor circuits.</summary>
public class DirectHttpClientFactory(HttpClientHandlerConfiguration? configurator = null) : HttpClientFactory(configurator)
{
	protected override HttpClientHandler CreateHttpClientHandler(string name)
	{
		var handler = base.CreateHttpClientHandler(name);
		handler.UseProxy = false;
		return handler;
	}
}

/// <summary>Only literal loopback addresses and localhost bypass the Tor proxy.</summary>
public sealed class LoopbackBypassProxy(Uri proxyUri, ICredentials credentials) : IWebProxy
{
	public ICredentials? Credentials { get; set; } = credentials;
	public Uri GetProxy(Uri destination) => IsBypassed(destination) ? destination : proxyUri;
	public bool IsBypassed(Uri destination) =>
		destination.IdnHost.Equals("localhost", StringComparison.OrdinalIgnoreCase) ||
		(IPAddress.TryParse(destination.IdnHost.Trim('[', ']'), out var address) &&
			IPAddress.IsLoopback(address.IsIPv4MappedToIPv6 ? address.MapToIPv4() : address));
}

public class CoordinatorHttpClientFactory : IMcwHttpClientFactory
{
	private readonly Uri _baseAddress;
	private readonly HttpClientFactory _internalHttpClientFactory;

	public CoordinatorHttpClientFactory(Uri baseAddress, HttpClientFactory internalHttpClientFactory)
	{
		_baseAddress = baseAddress;
		_internalHttpClientFactory = internalHttpClientFactory;
		_internalHttpClientFactory.AddLifetimeResolver(ResolveLifetimeByIdentity);
	}

	public HttpClient CreateClient(string name)
	{
		var httpClient = _internalHttpClientFactory.CreateClient(name);
		httpClient.BaseAddress = _baseAddress;
		httpClient.DefaultRequestVersion = HttpVersion.Version11;
		httpClient.DefaultRequestHeaders.UserAgent.Clear();
		return httpClient;
	}

	private DateTime ResolveLifetimeByIdentity(string name)
	{
		var identity = name.Split('-', StringSplitOptions.RemoveEmptyEntries).First();
		var lifetime = TimeSpan.FromSeconds(identity switch
		{
			"bob" => 40,
			"alice" => 1.5 * 3_600,
			"satoshi" => int.MaxValue,
			_ => int.MaxValue,
		});
		return DateTime.UtcNow.Add(lifetime);
	}
}

public class NotifyHttpClientHandler(string name, Action<string> disposedCallback) : HttpClientHandler
{
	protected override void Dispose(bool disposing)
	{
		Logger.LogDebug($"Disposing HTTP client handler {name}");
		base.Dispose(disposing);
		disposedCallback(name);
	}
}

public delegate Task<HttpResponseMessage> HttpSendCoreAsync(RetryHttpClientHandler handler, HttpRequestMessage request, CancellationToken cancellationToken);

public class RetryHttpClientHandler : NotifyHttpClientHandler
{
	private volatile bool _dispose;
	private readonly string _name;
	private readonly HttpClientHandlerConfiguration _config;
	private readonly HttpSendCoreAsync _send;
	private readonly Func<TimeSpan, CancellationToken, Task> _delay;
	private readonly TimeProvider _timeProvider;

	public RetryHttpClientHandler(string name, Action<string> disposedCallback, HttpClientHandlerConfiguration config,
		HttpSendCoreAsync? send = null, Func<TimeSpan, CancellationToken, Task>? delay = null, TimeProvider? timeProvider = null)
		: base(name, disposedCallback)
	{
		_config = config;
		_name = name;
		_send = send ?? SendCoreAsync;
		_delay = delay ?? Task.Delay;
		_timeProvider = timeProvider ?? TimeProvider.System;
	}

	protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
	{
		ObjectDisposedException.ThrowIf(_dispose, $"HTTP handler {_name} was already disposed.");

		for (var attempt = 0; attempt < _config.MaxAttempts; attempt++)
		{
			cancellationToken.ThrowIfCancellationRequested();
			if (_dispose)
			{
				throw new TimeoutException($"HTTP handler '{_name}' was disposed during request.");
			}

			TimeSpan retryDelay;
			try
			{
				var response = await _send(this, request, cancellationToken).ConfigureAwait(false);
				var baseDelay = response.StatusCode switch
				{
					HttpStatusCode.RequestTimeout or HttpStatusCode.BadGateway or HttpStatusCode.ServiceUnavailable => _config.TimeBeforeRetryingAfterServerError,
					HttpStatusCode.TooManyRequests => _config.TimeBeforeRetryingAfterTooManyRequests,
					_ => (TimeSpan?)null
				};
				if (baseDelay is null) { return response; }
				retryDelay = Backoff(baseDelay.Value, attempt);
				var retryAfter = response.Headers.RetryAfter;
				var serverDelay = retryAfter?.Delta ?? (retryAfter?.Date - _timeProvider.GetUtcNow());
				if (serverDelay is { } minimum && minimum > retryDelay) { retryDelay = minimum; }
				Logger.LogTrace($"Retrying {request.RequestUri} because {response.ReasonPhrase}");
				response.Dispose();
			}
			catch (OperationCanceledException)
			{
				throw;
			}
			catch (Exception e)
			{
				if (!ShouldRetry(e))
				{
					throw;
				}

				Logger.LogTrace($"Retrying {request.RequestUri} because {e.Message}");
				retryDelay = Backoff(_config.TimeBeforeRetryingAfterNetworkError, attempt);
			}
			if (attempt + 1 < _config.MaxAttempts) { await _delay(retryDelay, cancellationToken).ConfigureAwait(false); }
		}

		throw new HttpRequestException($"Failed to make http request '{request.RequestUri}' after {_config.MaxAttempts} attempts.");
	}

	private static TimeSpan Backoff(TimeSpan initial, int attempt) =>
		TimeSpan.FromSeconds(Math.Min(30, initial.TotalSeconds * Math.Pow(2, Math.Min(attempt, 6))));

	private async Task<HttpResponseMessage> SendCoreAsync(RetryHttpClientHandler handler, HttpRequestMessage request, CancellationToken cancellationToken)
	{
		return await base.SendAsync(request, cancellationToken).ConfigureAwait(false);
	}

	protected override void Dispose(bool disposing)
	{
		if (!_dispose)
		{
			_dispose = true;
			base.Dispose(disposing);
		}
	}

	private static bool ShouldRetry(Exception ex) =>
		ex switch
		{
			SocketException => true,
			HttpRequestException
			{
				HttpRequestError:
				HttpRequestError.ConnectionError or
				HttpRequestError.ProxyTunnelError or
				HttpRequestError.SecureConnectionError or
				HttpRequestError.NameResolutionError or
				HttpRequestError.InvalidResponse or
				HttpRequestError.ResponseEnded
			} => true,
			{ InnerException: Exception inner } when ShouldRetry(inner) => true,
			_ => false
		};
}
