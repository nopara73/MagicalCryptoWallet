using System.IO;
using System.Net;
using System.Net.Http;
using System.Net.Sockets;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Mcw.Network;
using MagicalCryptoWallet.WebClients.MagicalCryptoWallet;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.WebClients.MagicalCryptoWallet;

public class TorRoutingTests
{
	[Theory]
	[InlineData("http://localhost:38128/", true)]
	[InlineData("http://127.0.0.1:38128/", true)]
	[InlineData("http://[::1]:38128/", true)]
	[InlineData("https://mempool.space/api/tx/synthetic", false)]
	[InlineData("http://coordinator.onion/", false)]
	[InlineData("http://localhost.example.com/", false)]
	[InlineData("http://loopback/", true)] // Uri canonicalizes this alias to localhost.
	[InlineData("http://wallet.local/", false)]
	[InlineData("http://192.168.1.10/", false)]
	public void OnlyLoopbackBypassesTor(string address, bool bypass)
	{
		var proxyUri = new Uri("socks5://127.0.0.1:9050");
		var proxy = new LoopbackBypassProxy(proxyUri, new NetworkCredential("alice", "alice"));
		var destination = new Uri(address);
		Assert.Equal(bypass, proxy.IsBypassed(destination));
		Assert.Equal(bypass ? destination : proxyUri, proxy.GetProxy(destination));
	}

	[Fact]
	public void DirectPublicRequestsIgnoreSystemProxy()
	{
		using var handler = new InspectableDirectFactory().CreateHandler();
		Assert.False(handler.UseProxy);
	}

	[Fact]
	public async Task LoopbackRequestsWorkWhenTorIsUnavailableAsync()
	{
		using var cancellation = new CancellationTokenSource(TimeSpan.FromSeconds(15));
		using var listener = new TcpListener(IPAddress.Loopback, 0);
		listener.Start();
		var endpoint = (IPEndPoint)listener.LocalEndpoint;
		var server = RespondAsync(listener, cancellation.Token);
		IMcwHttpClientFactory factory = new OnionHttpClientFactory(new Uri("socks5://127.0.0.1:1"),
			new HttpClientHandlerConfiguration { MaxAttempts = 1 });
		using var client = factory.CreateClient("local-service");
		using var response = await client.GetAsync($"http://127.0.0.1:{endpoint.Port}/", cancellation.Token);
		Assert.Equal("synthetic", await response.Content.ReadAsStringAsync(cancellation.Token));
		await server;
	}

	[Fact]
	public async Task RemoteWalletRequestsUseSocksAndIdentityCredentialsAsync()
	{
		using var cancellation = new CancellationTokenSource(TimeSpan.FromSeconds(15));
		using var listener = new TcpListener(IPAddress.Loopback, 0);
		listener.Start();
		var endpoint = (IPEndPoint)listener.LocalEndpoint;
		var server = ServeSocksAsync(listener, cancellation.Token);
		IMcwHttpClientFactory factory = new OnionHttpClientFactory(new Uri($"socks5://127.0.0.1:{endpoint.Port}"),
			new HttpClientHandlerConfiguration { MaxAttempts = 1 });
		using var client = factory.CreateClient("alice-synthetic");
		using var response = await client.GetAsync("http://protected.invalid/tx/synthetic", cancellation.Token);
		Assert.Equal("synthetic", await response.Content.ReadAsStringAsync(cancellation.Token));
		var (identity, host) = await server;
		Assert.Equal("alice-synthetic", identity);
		Assert.Equal("protected.invalid", host);
	}

	private sealed class InspectableDirectFactory : DirectHttpClientFactory
	{
		public HttpClientHandler CreateHandler() => CreateHttpClientHandler("public-data");
	}

	private static async Task RespondAsync(TcpListener listener, CancellationToken cancellationToken)
	{
		using var socket = await listener.AcceptTcpClientAsync(cancellationToken);
		await SendHttpResponseAsync(socket.GetStream(), cancellationToken);
	}

	private static async Task<(string Identity, string Host)> ServeSocksAsync(TcpListener listener, CancellationToken cancellationToken)
	{
		using var socket = await listener.AcceptTcpClientAsync(cancellationToken);
		using var stream = socket.GetStream();
		var greeting = await ReadAsync(stream, 2, cancellationToken);
		Assert.Equal(5, greeting[0]);
		var methods = await ReadAsync(stream, greeting[1], cancellationToken);
		Assert.Contains((byte)2, methods);
		await stream.WriteAsync(new byte[] { 5, 2 }, cancellationToken);
		var auth = await ReadAsync(stream, 2, cancellationToken);
		Assert.Equal(1, auth[0]);
		var identity = Encoding.ASCII.GetString(await ReadAsync(stream, auth[1], cancellationToken));
		var passwordLength = (await ReadAsync(stream, 1, cancellationToken))[0];
		Assert.Equal(identity, Encoding.ASCII.GetString(await ReadAsync(stream, passwordLength, cancellationToken)));
		await stream.WriteAsync(new byte[] { 1, 0 }, cancellationToken);
		var request = await ReadAsync(stream, 4, cancellationToken);
		Assert.Equal(new byte[] { 5, 1, 0, 3 }, request);
		var hostLength = (await ReadAsync(stream, 1, cancellationToken))[0];
		var host = Encoding.ASCII.GetString(await ReadAsync(stream, hostLength, cancellationToken));
		await ReadAsync(stream, 2, cancellationToken);
		await stream.WriteAsync(new byte[] { 5, 0, 0, 1, 127, 0, 0, 1, 0, 80 }, cancellationToken);
		await SendHttpResponseAsync(stream, cancellationToken);
		return (identity, host);
	}

	private static async Task SendHttpResponseAsync(NetworkStream stream, CancellationToken cancellationToken)
	{
		using var reader = new StreamReader(stream, Encoding.ASCII, leaveOpen: true);
		while (await reader.ReadLineAsync(cancellationToken) is { Length: > 0 }) { }
		await stream.WriteAsync(Encoding.ASCII.GetBytes("HTTP/1.1 200 OK\r\nContent-Length: 9\r\nConnection: close\r\n\r\nsynthetic"), cancellationToken);
	}

	private static async Task<byte[]> ReadAsync(Stream stream, int count, CancellationToken cancellationToken)
	{
		var bytes = new byte[count];
		await stream.ReadExactlyAsync(bytes, cancellationToken);
		return bytes;
	}
}
