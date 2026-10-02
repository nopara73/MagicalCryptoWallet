using System.IO.Pipelines;
using System.Diagnostics;
using System.IO;
using MagicalCryptoWallet.Mcw.Network;
using System.Runtime.InteropServices;
using MagicalCryptoWallet.BundledApps;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Tor.Control;
using MagicalCryptoWallet.Tor.Control.Exceptions;
using MagicalCryptoWallet.Tor.Control.Messages;

namespace MagicalCryptoWallet.Tor;

public class TorProcessManager
{
	public TorProcessManager(TorSettings settings, EventBus eventBus, Func<PipeReader, CancellationToken, Task<TorControlReply>>? readReply = null)
	{
		_settings = settings;
		_eventBus = eventBus;
		_readReply = readReply ?? TorControlReplyReader.ReadReplyAsync;
	}

	private readonly TorSettings _settings;
	private readonly EventBus _eventBus;
	private readonly Func<PipeReader, CancellationToken, Task<TorControlReply>> _readReply;

	/// <param name="arguments">Command line arguments to start Tor OS process with.</param>
	public virtual Process StartProcess(string arguments)
	{
		ProcessStartInfo startInfo = new()
		{
			FileName = _settings.TorBinaryFilePath,
			Arguments = arguments,
			UseShellExecute = false,
			CreateNoWindow = true,
			RedirectStandardOutput = true,
			WorkingDirectory = _settings.TorBinaryDir
		};

		if (!RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
		{
			var env = startInfo.EnvironmentVariables;

			env["LD_LIBRARY_PATH"] = !env.ContainsKey("LD_LIBRARY_PATH") || string.IsNullOrEmpty(env["LD_LIBRARY_PATH"])
				? _settings.TorBinaryDir
				: _settings.TorBinaryDir + Path.PathSeparator + env["LD_LIBRARY_PATH"];

			Logger.LogDebug($"Environment variable 'LD_LIBRARY_PATH' set to: '{env["LD_LIBRARY_PATH"]}'.");
		}

		Logger.LogInfo($"Starting Tor from folder '{_settings.TorBinaryDir}' with arguments '{arguments}'…");

		var process = new Process()
		{
			StartInfo = startInfo,
		};

		process.StartWithExceptionLogging();

		return process;
	}

	public virtual async Task WaitForProcessExitAsync(Process process, CancellationToken cancellationToken)
	{
		await process.GracefulWaitForExitAsync(cancellationToken).ConfigureAwait(false);
	}

	public virtual void KillProcess(Process process)
	{
		process.Kill();
	}

	/// <summary>
	/// Connects to the Tor SOCKS5 proxy and starts the handshaking process to find out if Tor is up and ready.
	/// </summary>
	public virtual async Task<bool> IsTorRunningAsync(CancellationToken cancellationToken)
	{
		try
		{
			var result = await McwSocksProbe.CheckAsync(_settings.SocksEndpoint, cancellationToken).ConfigureAwait(false);
			if (!result.IsReady)
			{
				Logger.LogInfo($"Tor SOCKS5 readiness probe failed: {result.Failure}.");
			}
			_eventBus.Publish(new TorConnectionStateChanged(result.IsReady));
			return result.IsReady;
		}
		catch (IOException)
		{
			Logger.LogInfo("Tor SOCKS5 readiness service is unavailable.");
			_eventBus.Publish(new TorConnectionStateChanged(false));
			return false;
		}
	}

	/// <summary>
	/// Ensure <paramref name="process"/> is actually running.
	/// </summary>
	public virtual async Task<bool> EnsureRunningAsync(Process process, CancellationToken token)
	{
		int i = 0;
		while (true)
		{
			i++;

			bool isRunning = await IsTorRunningAsync(token).ConfigureAwait(false);

			if (isRunning)
			{
				return true;
			}

			if (process.HasExited)
			{
				Logger.LogError("Tor process failed to start!");
				return false;
			}

			const int MaxAttempts = 25;

			if (i >= MaxAttempts)
			{
				Logger.LogError($"All {MaxAttempts} attempts to connect to Tor failed.");
				return false;
			}

			// Wait 250 milliseconds between attempts.
			await Task.Delay(250, token).ConfigureAwait(false);
		}
	}

	public virtual Process[] GetTorProcesses()
	{
		return Process.GetProcessesByName(TorSettings.TorBinaryFileName);
	}

	/// <summary>
	/// Connects to Tor control using a TCP client or throws <see cref="TorControlException"/>.
	/// </summary>
	/// <exception cref="TorControlException">When authentication fails for some reason.</exception>
	/// <seealso href="https://gitweb.torproject.org/torspec.git/tree/control-spec.txt">This method follows instructions in 3.23. TAKEOWNERSHIP.</seealso>
	public virtual async Task<TorControlClient> InitTorControlAsync(CancellationToken token)
	{
		// If the cookie file does not exist, we know our Tor starting procedure is corrupted somehow. Best to start from scratch.
		if (!File.Exists(_settings.CookieAuthFilePath))
		{
			throw new TorControlException("Cookie file does not exist.");
		}

		// Get cookie.
		string cookieString = Convert.ToHexString(File.ReadAllBytes(_settings.CookieAuthFilePath));

		// Authenticate.
		TorControlClientFactory factory = new(RandomnessProviders.Secure, _readReply);
		TorControlClient client = await factory.ConnectAndAuthenticateAsync(_settings.ControlEndpoint, cookieString, token).ConfigureAwait(false);

		if (_settings.TerminateOnExit)
		{
			// This is necessary for the scenario when Tor was started by a previous WW instance with TerminateTorOnExit=false configuration option.
			TorControlReply takeReply = await client.TakeOwnershipAsync(token).ConfigureAwait(false);

			if (!takeReply)
			{
				throw new TorControlException($"Failed to take ownership of the Tor instance. Reply: '{takeReply}'.");
			}

			TorControlReply resetReply = await client.ResetOwningControllerProcessConfAsync(token).ConfigureAwait(false);

			if (!resetReply)
			{
				throw new TorControlException($"Failed to reset __OwningControllerProcess. Reply: '{resetReply}'.");
			}
		}

		return client;
	}
}
