using ReactiveUI;
using System.Windows.Input;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.ViewModels;
using MagicalCryptoWallet.Fluent.ViewModels.HelpAndSupport;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Fluent.CrashReport.ViewModels;

public class CrashReportWindowViewModel : ViewModelBase
{
	public CrashReportWindowViewModel(UiContext uiContext, SerializableException serializedException) : base(uiContext)
	{
		SerializedException = serializedException;
		CancelCommand = ReactiveCommand.Create(() => AppLifetimeHelper.Shutdown(withShutdownPrevention: false, restart: true));
		NextCommand = ReactiveCommand.Create(() => AppLifetimeHelper.Shutdown(withShutdownPrevention: false, restart: false));

		OpenGitHubRepoCommand = ReactiveCommand.CreateFromTask(async () => await IoHelpers.OpenBrowserAsync(Link));

		CopyTraceCommand = ReactiveCommand.CreateFromTask(async () => await ApplicationHelper.SetTextAsync(Trace));
	}

	public SerializableException SerializedException { get; }

	public ICommand OpenGitHubRepoCommand { get; }

	public ICommand NextCommand { get; }

	public ICommand CancelCommand { get; }

	public ICommand CopyTraceCommand { get; }

	public string Caption => $"A problem has occurred and Magical Crypto Wallet is unable to continue.";

	public string Link => AboutViewModel.BugReportLink;

	public string Trace => SerializedException.ToString();

	public string Title => "Magical Crypto Wallet has crashed";
}
