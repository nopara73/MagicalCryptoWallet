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
		var hash = SHA256.HashData(Encoding.UTF8.GetBytes(Environment.UserName + "\0" + path));
		// Keep all 128 identity bits while fitting macOS's long per-user temporary directory.
		return "mcw-" + Convert.ToBase64String(hash[..16]).TrimEnd('=').Replace('+', '-').Replace('/', '_');
	}
	public void Bind(Action show)
	{
		bool pending;
		lock (_gate) { _show = show; pending = _pending; _pending = false; }
		if (pending) { show(); }
	}
	private async Task ListenAsync(CancellationToken cancel)
	{
		using var server = new NamedPipeServerStream(_pipeName, PipeDirection.InOut, 1,
			PipeTransmissionMode.Byte, PipeOptions.Asynchronous | PipeOptions.CurrentUserOnly);
		while (!cancel.IsCancellationRequested)
		{
			bool connected = false;
			try
			{
				await server.WaitForConnectionAsync(cancel).ConfigureAwait(false);
				connected = true;
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
				// Do not close the peer before it finishes credential checks and reads our acknowledgement.
				await server.ReadExactlyAsync(new byte[1], requestTimeout.Token).ConfigureAwait(false);
			}
			catch (OperationCanceledException) when (cancel.IsCancellationRequested) { break; }
			catch (Exception ex) when (ex is IOException or OperationCanceledException) { Logger.LogDebug(ex); }
			finally { if (connected) { server.Disconnect(); } }
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
			await client.WriteAsync(response, timeout.Token).ConfigureAwait(false);
			return response[0] == 1;
		}
		catch (Exception ex) when (ex is IOException or OperationCanceledException or UnauthorizedAccessException) { Logger.LogInfo(ex); return false; }
	}
	public async ValueTask DisposeAsync()
	{
		await _stopping.CancelAsync().ConfigureAwait(false);
		await _listener.ConfigureAwait(false);
		_stopping.Dispose();
	}
}
