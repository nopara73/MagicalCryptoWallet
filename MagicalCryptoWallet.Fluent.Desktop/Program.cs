using System.Diagnostics;
using Avalonia;
using Avalonia.Controls;
using System.IO;
using System.Reactive;
using System.Reactive.Concurrency;
using System.Runtime.InteropServices;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Threading;
using ReactiveUI;
using System.Linq;
using MagicalCryptoWallet.Fluent.CrashReport;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Fluent.Desktop.Extensions;
using System.Net.Sockets;
using System.Collections.ObjectModel;
using LogLevel = MagicalCryptoWallet.Logging.LogLevel;
using System.Threading;
using MagicalCryptoWallet.Services;
using ReactiveUI.Avalonia;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Client.Application;

namespace MagicalCryptoWallet.Fluent.Desktop;

public class Program
{
	private static int IsShuttingDownFlag;

	internal static bool IsShuttingDown => Volatile.Read(ref IsShuttingDownFlag) == 1;

	internal static bool IsDbusMenuShutdownException(Exception exception)
	{
		if (exception is not NullReferenceException)
		{
			return false;
		}

		var declaringType = exception.TargetSite?.DeclaringType?.FullName;
		if (declaringType?.Contains("Avalonia.FreeDesktop.DBusMenuExporter", StringComparison.Ordinal) == true)
		{
			return true;
		}

		return exception.StackTrace?.Contains("Avalonia.FreeDesktop.DBusMenuExporter", StringComparison.Ordinal) == true;
	}

	// Initialization code. Don't use any Avalonia, third-party APIs or any
	// SynchronizationContext-reliant code before AppMain is called: things aren't initialized
	// yet and stuff might break.
	[STAThread]
	public static int Main(string[] args)
	{
		if (ManagedApplicationHost.TryDelegate("gui", args, out var delegatedExitCode))
		{
			return delegatedExitCode;
		}
		using var host = ManagedApplicationHost.Connect();
		if (host.StartupArguments.Length != 0) { args = host.StartupArguments; }
		MagicalCryptoWallet.Helpers.WindowsAppIdentity.Apply();
		// Crash reporting must be before the "single instance checking".
		if (CrashReporter.TryGetExceptionFromCliArgs(args, out var exceptionToShow))
		{
			host.BindShutdown(TerminateApplication);
			Logger.Configure(Path.Combine(Config.DataDir, "Logs.txt"), LogLevel.Info);

			try
			{
				// Show the exception.
				BuildCrashReporterApp(exceptionToShow).StartWithClassicDesktopLifetime(args);
				return 1;
			}
			catch (Exception ex)
			{
				// If anything happens here just log it and exit.
				Logger.LogCritical(ex);
				return 1;
			}
		}

		try
		{
			var app = MagicalCryptoWalletAppBuilder
				.Create("Magical Crypto Wallet GUI", args)
				.EnsureSingleInstance()
				.WithDesktopActivation()
				.OnUnhandledExceptions(LogUnhandledException)
				.OnUnobservedTaskExceptions(LogUnobservedTaskException)
				.OnTermination(TerminateApplication)
				.Build();

			host.BindTermination(app.TerminateService);
			var exitCode = app.RunAsGui();

			if (app.TerminateService.GracefulCrashException is not null)
			{
				throw app.TerminateService.GracefulCrashException;
			}

			if (exitCode == ExitCode.Ok && app.HasStarted && app.Global is {Status: {InstallOnClose: true, InstallerFilePath: var installerFilePath}})
			{
				host.Handoff(ManagedApplicationHost.UpdateOperation, [Path.GetFullPath(installerFilePath)]);
			}

			else if (app.HasStarted && AppLifetimeHelper.RestartRequested)
			{
				var launch = new StartupLaunch(EnvironmentHelpers.GetExecutablePath(), app.DataDirectory, app.Config.Network);
				var preserved = app.AppConfig.Arguments.Where(argument => argument != StartupHelper.SilentArgument && !argument.StartsWith("--datadir=", StringComparison.OrdinalIgnoreCase) && !argument.StartsWith("--network=", StringComparison.OrdinalIgnoreCase));
				host.Handoff(ManagedApplicationHost.RestartOperation, preserved.Concat(launch.Arguments(silent: false)).ToArray());
			}
			return (int)exitCode;
		}
		catch (Exception ex)
		{
			CrashReporter.Invoke(ex);
			Logger.LogCritical(ex);
			return 1;
		}
	}

	/// <summary>
	/// Do not call this method it should only be called by TerminateService.
	/// </summary>
	private static void TerminateApplication()
	{
		Interlocked.Exchange(ref IsShuttingDownFlag, 1);
		if (Application.Current is null)
		{
			return;
		}

		Dispatcher.UIThread.Post(() =>
		{
			if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
			{
				DetachTrayIconMenus();
			}

			(Application.Current.ApplicationLifetime as IClassicDesktopStyleApplicationLifetime)?.Shutdown();
		}, DispatcherPriority.Send);
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

	private static void DetachTrayIconMenus()
	{
		if (Application.Current is not { } app)
		{
			return;
		}

		var trayIcons = TrayIcon.GetIcons(app);
		if (trayIcons is null)
		{
			return;
		}

		foreach (var icon in trayIcons)
		{
			// Detach the menu before shutdown to prevent DBus menu updates after disposal.
			icon.Menu = null;
		}
	}

	/// <summary>
	/// Sets up and initializes the crash reporting UI.
	/// </summary>
	/// <param name="serializableException">The serializable exception</param>
	private static AppBuilder BuildCrashReporterApp(SerializableException serializableException)
	{
		var result = AppBuilder
			.Configure(() => new CrashReportApp(serializableException))
			.UseReactiveUI();

		if (RuntimeInformation.IsOSPlatform(OSPlatform.Windows))
		{
			result
				.UseWin32()
				.UseSkia();
		}
		else
		{
			result.UsePlatformDetect();
		}

		return result
			.With(new Win32PlatformOptions { RenderingMode = new[] { Win32RenderingMode.Software } })
			.With(new X11PlatformOptions { RenderingMode = new[] { X11RenderingMode.Software }, WmClass = "Magical Crypto Wallet Crash Report" })
			.With(new AvaloniaNativePlatformOptions { RenderingMode = new[] { AvaloniaNativeRenderingMode.Software } })
			.With(new MacOSPlatformOptions { ShowInDock = true })
			.AfterSetup(_ => ThemeHelper.ApplyTheme(Theme.Dark));
	}
}

public static class MagicalCryptoWalletAppExtensions
{
	public static ExitCode RunAsGui(this MagicalCryptoWalletApplication app)
	{
		return app.Run(afterStarting: () =>
			{
				RxApp.DefaultExceptionHandler = Observer.Create<Exception>(ex =>
				{
					if (Debugger.IsAttached)
					{
						Debugger.Break();
					}

					Logger.LogError(ex);

					RxApp.MainThreadScheduler.Schedule(() => throw new ApplicationException("Exception has been thrown in unobserved ThrownExceptions", ex));
				});

				Logger.LogInfo("Magical Crypto Wallet GUI started.");
				bool runGuiInBackground = app.AppConfig.Arguments.Contains(StartupHelper.SilentArgument, StringComparer.Ordinal);
				UiConfig uiConfig = LoadOrCreateUiConfig(app.DataDirectory);
				var services = Services.Create(app.Global, uiConfig, app.TerminateService);

				using CancellationTokenSource stopLoadingCts = new();

				AppBuilder appBuilder = AppBuilder.Configure(() => new App(
					backendInitializeAsync: async () =>
					{
						// macOS require that Avalonia is started with the UI thread. Hence this call must be delayed to this point.
						await app.Global.InitializeAsync(initializeSleepInhibitor: true, app.TerminateService, stopLoadingCts.Token).ConfigureAwait(false);

						// Make sure that wallet startup set correctly regarding RunOnSystemStartup
						if (uiConfig.RunOnSystemStartup)
						{
							await StartupHelper.ModifyStartupSettingAsync(true, new StartupLaunch(EnvironmentHelpers.GetExecutablePath(), app.DataDirectory, app.Config.Network)).ConfigureAwait(false);
						}
					}, startInBg: runGuiInBackground, activation: app.Activation))
					.UseReactiveUI()
					.SetupAppBuilder(services.Config.EnableGpu)
					.AfterSetup(_ =>
					{
						ThemeHelper.ApplyTheme(uiConfig.DarkModeEnabled ? Theme.Dark : Theme.Light);

						if (RuntimeInformation.IsOSPlatform(OSPlatform.Linux))
						{
							Dispatcher.UIThread.UnhandledException += (_, e) =>
							{
								if (Program.IsShuttingDown &&
									Program.IsDbusMenuShutdownException(e.Exception))
								{
									Logger.LogWarning("Suppressing DBusMenuExporter exception during shutdown.");
									e.Handled = true;
								}
							};
						}
					});

				if (app.TerminateService.CancellationToken.IsCancellationRequested)
				{
					Logger.LogDebug("Skip starting Avalonia UI as requested the application to stop.");
					stopLoadingCts.Cancel();
				}
				else
				{
					appBuilder.StartWithClassicDesktopLifetime(app.AppConfig.Arguments);
				}
			});
	}

	private static UiConfig LoadOrCreateUiConfig(string dataDir)
	{
		Directory.CreateDirectory(dataDir);

		return UiConfig.LoadFile(Path.Combine(dataDir, "UiConfig.json"));
	}
}
