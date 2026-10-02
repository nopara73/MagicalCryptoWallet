using System;
using System.IO;
using System.Linq;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Services.Terminate;
using Constants = MagicalCryptoWallet.Helpers.Constants;

namespace MagicalCryptoWallet.Client;

public class MagicalCryptoWalletApplication
{
	public MagicalCryptoWalletAppBuilder AppConfig { get; }
	private Global? _global;
	public Global Global => _global ?? throw new InvalidOperationException("The application has not acquired its data-directory lock.");
	public bool HasStarted => _global is not null;
	public string DataDirectory { get; }
	public DesktopActivation? Activation { get; private set; }
	private Config? _config;
	public Config Config => _config ?? throw new InvalidOperationException("Configuration is not initialized.");
	public SingleInstanceChecker SingleInstanceChecker { get; }
	public TerminateService TerminateService { get; }
	private static Guid InstanceGuid { get; } = Guid.NewGuid();

	public MagicalCryptoWalletApplication(MagicalCryptoWalletAppBuilder magicalcryptowalletAppBuilder)
	{
		AppConfig = magicalcryptowalletAppBuilder;

		CheckVersionAndHelp();
		DataDirectory = Configuration.Config.ResolveDataDirectory(AppConfig.Arguments);
		Directory.CreateDirectory(DataDirectory);
		SingleInstanceChecker = new(DataDirectory);
		TerminateService = new(TerminateApplicationAsync, AppConfig.Terminate);
	}

	private void CheckVersionAndHelp()
	{
		if (AppConfig.Arguments.Contains("--version"))
		{
			Console.WriteLine($"{AppConfig.AppName} {Constants.ClientVersion}");
			Environment.Exit((int)ExitCode.Ok);
		}

		if (AppConfig.Arguments.Contains("--help") || AppConfig.Arguments.Contains("-h"))
		{
			ShowHelp();
			Environment.Exit((int)ExitCode.Ok);
		}

	}

	public ExitCode Run(Action afterStarting)
	{
		var exitCode = ProcessAppArguments();
		if (exitCode is not null)
		{
			return exitCode.Value;
		}

		try
		{
			TerminateService.Activate();
			BeforeStarting();

			afterStarting();
			return ExitCode.Ok;
		}
		catch (Exception e)
		{
			Logger.LogInfo("Exception occurred while the application was starting or running", e);
			throw;
		}
		finally
		{
			BeforeStopping();
		}
	}

	public async Task<ExitCode> RunAsync(Func<Task> afterStarting)
	{
		var exitCode = ProcessAppArguments();
		if (exitCode is not null)
		{
			return exitCode.Value;
		}

		try
		{
			TerminateService.Activate();
			BeforeStarting();

			await afterStarting();
			return ExitCode.Ok;
		}
		catch (Exception e)
		{
			Logger.LogInfo("Exception occurred while the application was starting or running", e);
			throw;
		}
		finally
		{
			BeforeStopping();
		}
	}

	private ExitCode? ProcessAppArguments()
	{
		if (AppConfig.MustCheckSingleInstance && !SingleInstanceChecker.IsFirstInstance())
		{
			var network = ResolveRequestedNetwork();
			if (AppConfig.IsDesktop && DesktopActivation.RequestAsync(DataDirectory, network,
				AppConfig.Arguments.Contains("startsilent", StringComparer.Ordinal)).GetAwaiter().GetResult()) { return ExitCode.Ok; }
			Console.Error.WriteLine("This data directory is already in use by another process or network.");
			return ExitCode.FailedAlreadyRunningError;
		}
		try
		{
			SetupLogger();
			_config = new Config(LoadOrCreateConfigs(), AppConfig.Arguments);
			_global = new Global(DataDirectory, Config);
			if (AppConfig.IsDesktop) { Activation = new DesktopActivation(DataDirectory, Config.Network); }
			return null;
		}
		catch { SingleInstanceChecker.Dispose(); throw; }
	}

	private Network ResolveRequestedNetwork()
	{
		var fallback = File.Exists(Path.Combine(DataDirectory, "network")) ? File.ReadAllText(Path.Combine(DataDirectory, "network")).Trim() : "Main";
		return Configuration.Config.ResolveNetwork(AppConfig.Arguments, fallback);
	}

	private void BeforeStarting()
	{
		AppDomain.CurrentDomain.UnhandledException += CurrentDomain_UnhandledException;
		TaskScheduler.UnobservedTaskException += TaskScheduler_UnobservedTaskException;

		Logger.LogInfo($"{AppConfig.AppName} started ({InstanceGuid}).", callerFilePath: "", callerLineNumber: -1);
	}

	private void BeforeStopping()
	{
		AppDomain.CurrentDomain.UnhandledException -= CurrentDomain_UnhandledException;
		TaskScheduler.UnobservedTaskException -= TaskScheduler_UnobservedTaskException;

		// Start termination/disposal of the application.
		TerminateService.Terminate();
		SingleInstanceChecker.Dispose();
		Logger.LogInfo($"{AppConfig.AppName} stopped gracefully ({InstanceGuid}).", callerFilePath: "", callerLineNumber: -1);
	}

	private PersistentConfig LoadOrCreateConfigs()
	{
		CreateConfigFiles();

		var networkFilePath = Path.Combine(DataDirectory, "network");
		Logger.LogInfo($"Loading network file '{networkFilePath}'.");

		var network = ResolveRequestedNetwork();
		var networkName = network.Name;
		if (!File.Exists(networkFilePath)) { PersistentConfigManager.UpdateNetwork(networkFilePath, network); }
		var configFileName = networkName switch
		{
			_ when network == Network.Main => "Config.json",
			_ when network == Network.TestNet => "Config.TestNet.json",
			_ when network == Network.RegTest => "Config.RegTest.json",
			_ when network == Bitcoin.Instance.Signet => "Config.Signet.json",
			_ => throw new NotSupportedException($"Network '{networkName}' is not supported."),
		};
		var configFilePath = Path.Combine(DataDirectory, configFileName);

		Logger.LogInfo($"Loading config file '{configFilePath}'.");
		var persistentConfig = PersistentConfigManager.LoadFile(configFilePath);

		if (persistentConfig is PersistentConfig config)
		{
			var configForNetwork = config with { Network = network };
			return configForNetwork;
		}

		throw new InvalidOperationException("Unknown configuration type");
	}

	private void CreateConfigFiles()
	{
		CreateConfigFileIfNotExists(Path.Combine(DataDirectory, "Config.RegTest.json"),
			PersistentConfigManager.DefaultRegTestConfig);
		CreateConfigFileIfNotExists(Path.Combine(DataDirectory, "Config.TestNet.json"),
			PersistentConfigManager.DefaultTestNetConfig);
		CreateConfigFileIfNotExists(Path.Combine(DataDirectory, "Config.Signet.json"),
			PersistentConfigManager.DefaultSignetConfig);
		CreateConfigFileIfNotExists(Path.Combine(DataDirectory, "Config.json"),
			PersistentConfigManager.DefaultMainNetConfig);
		return;

		static void CreateConfigFileIfNotExists(string filePath, PersistentConfig config)
		{
			if (!File.Exists(filePath))
			{
				PersistentConfigManager.ToFile(filePath, config);
			}
		}
	}

	private void TaskScheduler_UnobservedTaskException(object? sender, UnobservedTaskExceptionEventArgs e)
	{
		AppConfig.UnobservedTaskExceptionsEventHandler?.Invoke(this, e.Exception);
	}

	private void CurrentDomain_UnhandledException(object? sender, UnhandledExceptionEventArgs e)
	{
		if (e.ExceptionObject is Exception ex)
		{
			AppConfig.UnhandledExceptionEventHandler?.Invoke(this, ex);
		}
	}

	private async Task TerminateApplicationAsync()
	{
		Logger.LogInfo($"{AppConfig.AppName} stopped gracefully ({InstanceGuid}).", callerFilePath: "", callerLineNumber: -1);

		if (_global is { } global) { await global.DisposeAsync().ConfigureAwait(false); }
		if (Activation is { } activation) { await activation.DisposeAsync().ConfigureAwait(false); }
	}

	private void SetupLogger()
	{
		LogLevel logLevel = Enum.TryParse(Config.LogLevel, ignoreCase: true, out LogLevel parsedLevel)
			? parsedLevel
			: LogLevel.Info;

		Logger.Configure(Path.Combine(DataDirectory, "Logs.txt"), logLevel, Config.LogModes);
	}

	private void ShowHelp()
	{
		Console.WriteLine($"{AppConfig.AppName} {Constants.ClientVersion}");
		Console.WriteLine($"Usage: {AppConfig.AppName} [OPTION]...");
		Console.WriteLine();
		Console.WriteLine("Available options are:");

		foreach (var (parameter, hint) in Config.GetConfigOptionsMetadata().OrderBy(x => x.ParameterName))
		{
			Console.Write($"  --{parameter.ToLower(),-30} ");
			var hintLines = hint.SplitLines(lineWidth: 40);
			Console.WriteLine(hintLines[0]);
			foreach (var hintLine in hintLines.Skip(1))
			{
				Console.WriteLine($"{' ',-35}{hintLine}");
			}
			Console.WriteLine();
		}
	}
}
