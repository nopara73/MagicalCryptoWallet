using System;
using System.IO;
using System.IO.Pipes;
using System.Security.Cryptography;
using System.Text;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.Client;

/// <summary>A current-user-only activation channel. It cannot load wallets or issue application commands.</summary>
public sealed class DesktopActivation : IAsyncDisposable
{
	private readonly object _gate = new();
	private readonly CancellationTokenSource _stopping = new();
	private readonly string _pipeName;
	private readonly string _network;
	private readonly Task _listener;
	private Action? _show;
	private bool _pending;

	public DesktopActivation(string directory, Network network)
	{
		_pipeName = GetPipeName(directory);
		_network = network.Name;
		_listener = ListenAsync(_stopping.Token);
	}
	private static string GetPipeName(string directory)
	{
		var path = Path.TrimEndingDirectorySeparator(Path.GetFullPath(directory));
		if (OperatingSystem.IsWindows()) { path = path.ToUpperInvariant(); }
		return "mcw-activate-" + Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(Environment.UserName + "\0" + path)))[..32];
	}
	public void Bind(Action show)
	{
		bool pending;
		lock (_gate) { _show = show; pending = _pending; _pending = false; }
		if (pending) { show(); }
	}
	private async Task ListenAsync(CancellationToken cancel)
	{
		while (!cancel.IsCancellationRequested)
		{
			try
			{
				using var server = new NamedPipeServerStream(_pipeName, PipeDirection.InOut, 1,
					PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
				await server.WaitForConnectionAsync(cancel).ConfigureAwait(false);
				using var requestTimeout = CancellationTokenSource.CreateLinkedTokenSource(cancel);
				requestTimeout.CancelAfter(TimeSpan.FromSeconds(2));
				var request = new byte[64];
				await server.ReadExactlyAsync(request, requestTimeout.Token).ConfigureAwait(false);
				var message = Encoding.UTF8.GetString(request).TrimEnd('\0');
				var valid = message == "show:" + _network || message == "silent:" + _network;
				if (valid && message.StartsWith("show:", StringComparison.Ordinal))
				{
					Action? show;
					lock (_gate) { show = _show; if (show is null) { _pending = true; } }
					show?.Invoke();
				}
				await server.WriteAsync(new byte[] { valid ? (byte)1 : (byte)0 }, requestTimeout.Token).ConfigureAwait(false);
			}
			catch (OperationCanceledException) when (cancel.IsCancellationRequested) { break; }
			catch (Exception ex) when (ex is IOException or OperationCanceledException) { Logger.LogDebug(ex); }
		}
	}
	public static async Task<bool> RequestAsync(string directory, Network network, bool silent)
	{
		using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(2));
		try
		{
			using var client = new NamedPipeClientStream(".", GetPipeName(directory), PipeDirection.InOut,
				PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
			await client.ConnectAsync(timeout.Token).ConfigureAwait(false);
			var message = Encoding.UTF8.GetBytes((silent ? "silent:" : "show:") + network.Name);
			var packet = new byte[64];
			message.CopyTo(packet, 0);
			await client.WriteAsync(packet, timeout.Token).ConfigureAwait(false);
			var response = new byte[1];
			await client.ReadExactlyAsync(response, timeout.Token).ConfigureAwait(false);
			return response[0] == 1;
		}
		catch (Exception ex) when (ex is IOException or OperationCanceledException or UnauthorizedAccessException) { return false; }
	}
	public async ValueTask DisposeAsync()
	{
		await _stopping.CancelAsync().ConfigureAwait(false);
		await _listener.ConfigureAwait(false);
		_stopping.Dispose();
	}
}
