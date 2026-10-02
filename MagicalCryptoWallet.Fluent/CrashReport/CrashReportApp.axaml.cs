using Avalonia;
using Avalonia.Controls.ApplicationLifetimes;
using Avalonia.Markup.Xaml;
using MagicalCryptoWallet.Fluent.CrashReport.ViewModels;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Fluent.CrashReport.Views;

namespace MagicalCryptoWallet.Fluent.CrashReport;

public class CrashReportApp : Application
{
	private readonly SerializableException? _serializableException;

	public CrashReportApp()
	{
		Name = "Magical Crypto Wallet Crash Report";
	}

	public CrashReportApp(SerializableException exception) : this()
	{
		_serializableException = exception;
	}

	public override void Initialize()
	{
		AvaloniaXamlLoader.Load(this);
	}

	public override void OnFrameworkInitializationCompleted()
	{
		if (ApplicationLifetime is IClassicDesktopStyleApplicationLifetime desktop && _serializableException is { })
		{
			desktop.MainWindow = new CrashReportWindow
			{
				DataContext = new CrashReportWindowViewModel(null!, _serializableException)
			};
		}

		base.OnFrameworkInitializationCompleted();
	}
}
