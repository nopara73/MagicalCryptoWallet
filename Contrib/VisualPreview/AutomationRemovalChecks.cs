using System.Reactive.Linq;
using System.Reactive.Subjects;
using System.Reflection;
using Avalonia;
using Avalonia.Styling;
using Avalonia.Threading;
using DynamicData;
using MagicalCryptoWallet.Fluent;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;
using MagicalCryptoWallet.Fluent.ViewModels.SearchBar.Sources;

internal static class AutomationRemovalChecks
{
	public static void Run(UiContext context)
	{
		// Use the application's fresh generated metadata and actual action-search source.
		foreach (var type in typeof(App).Assembly.GetTypes())
		{
			if (type.GetProperty("MetaData", BindingFlags.Public | BindingFlags.Static | BindingFlags.DeclaredOnly)?.GetValue(null) is NavigationMetaData metadata &&
				(!metadata.Searchable || metadata.Category is not null))
			{
				NavigationManager.RegisterLazy(metadata, () => null);
			}
		}
		foreach (var theme in new[] { ThemeVariant.Light, ThemeVariant.Dark })
		{
			Application.Current!.RequestedThemeVariant = theme;
			using var query = new BehaviorSubject<string>("");
			var source = new ActionsSearchSource(context, query);
			using var changes = source.Changes.Bind(out var results).Subscribe();
			Dispatcher.UIThread.RunJobs();
			Check(results.Count > 0, "Action search must retain desktop actions.");
			Check(!NavigationManager.MetaData.Any(m => m.Title.Contains("Scripting", StringComparison.OrdinalIgnoreCase)), "Navigation must not register the retired action.");
			foreach (var text in new[] { "Scripting", "Scheme", "Automate Magical Crypto Wallet" })
			{
				query.OnNext(text);
				Dispatcher.UIThread.RunJobs();
				Check(results.Count == 0, $"Action search returned the retired action for {text} in {theme.Key}.");
			}
		}
		Console.WriteLine("Desktop action search and generated navigation contain no scripting action or replacement in either theme.");
	}

	private static void Check(bool condition, string message)
	{
		if (!condition) { throw new InvalidOperationException(message); }
	}
}
