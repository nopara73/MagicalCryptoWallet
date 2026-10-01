using System;
using System.Collections.ObjectModel;
using System.Net.Sockets;
using System.Threading.Tasks;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.Daemon;

public class Program
{
	public static async Task<int> Main(string[] args)
	{
		var app = MagicalCryptoWalletAppBuilder
			.Create("Magical Crypto Wallet Daemon", args)
			.EnsureSingleInstance()
			.OnUnhandledExceptions(LogUnhandledException)
			.OnUnobservedTaskExceptions(LogUnobservedTaskException)
			.Build();

		var exitCode = await app.RunAsConsoleAsync().ConfigureAwait(false);
		return (int)exitCode;
	}

	private static void LogUnobservedTaskException(object? sender, AggregateException e)
	{
		ReadOnlyCollection<Exception> innerExceptions = e.Flatten().InnerExceptions;

		switch (innerExceptions)
		{
			case [SocketException { SocketErrorCode: SocketError.OperationAborted }]:
			// Source of this exception is NBitcoin library.
			case [OperationCanceledException { Message: "The peer has been disconnected" }]:
				// Until https://github.com/MetacoSA/NBitcoin/pull/1089 is resolved.
				Logger.LogTrace(e);
				break;
			default:
				Logger.LogDebug(e);
				break;
		}
	}

	private static void LogUnhandledException(object? sender, Exception e) =>
		Logger.LogWarning(e);
}

public static class MagicalCryptoWalletAppExtensions
{
	public static async Task<ExitCode> RunAsConsoleAsync(this MagicalCryptoWalletApplication app)
	{

		return await app.RunAsync(
			async () =>
			{
				try
				{
					await app.Global.InitializeAsync(initializeSleepInhibitor: false, app.TerminateService, app.TerminateService.CancellationToken).ConfigureAwait(false);
				}
				catch (OperationCanceledException) when (app.TerminateService.CancellationToken.IsCancellationRequested)
				{
					Logger.LogInfo("User requested the application to stop. Stopping.");
				}

				if (!app.TerminateService.CancellationToken.IsCancellationRequested)
				{
					if (app.Global.WalletManager.GetWallet() is { } wallet)
					{
						await app.Global.WalletManager.StartWalletAsync(wallet).ConfigureAwait(false);
					}
					await app.TerminateService.ForcefulTerminationRequestedTask.ConfigureAwait(false);
				}
			}).ConfigureAwait(false);
	}
}
