using System;

namespace MagicalCryptoWallet.Client;

public enum ExitCode
{
	Ok,
	FailedAlreadyRunningSignaled,
	FailedAlreadyRunningError,
}

public record MagicalCryptoWalletAppBuilder(string AppName, string[] Arguments)
{
	internal bool MustCheckSingleInstance { get; init; }
	internal bool IsDesktop { get; init; }
	internal EventHandler<Exception>? UnhandledExceptionEventHandler { get; init; }
	internal EventHandler<AggregateException>? UnobservedTaskExceptionsEventHandler { get; init; }
	internal Action Terminate { get; init; } = () => { };

	public MagicalCryptoWalletAppBuilder EnsureSingleInstance(bool ensure = true) =>
		this with { MustCheckSingleInstance = ensure };
	public MagicalCryptoWalletAppBuilder WithDesktopActivation() => this with { IsDesktop = true };

	public MagicalCryptoWalletAppBuilder OnUnhandledExceptions(EventHandler<Exception> handler) =>
		this with { UnhandledExceptionEventHandler = handler };

	public MagicalCryptoWalletAppBuilder OnUnobservedTaskExceptions(EventHandler<AggregateException> handler) =>
		this with { UnobservedTaskExceptionsEventHandler = handler };

	public MagicalCryptoWalletAppBuilder OnTermination(Action action) =>
		this with { Terminate = action };
	public MagicalCryptoWalletApplication Build() =>
		new(this);

	public static MagicalCryptoWalletAppBuilder Create(string appName, string[] args) =>
		new(appName, args);
}
