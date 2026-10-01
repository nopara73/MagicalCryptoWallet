using System.Reactive.Linq;
using ReactiveUI;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.ViewModels.Navigation;

namespace MagicalCryptoWallet.Fluent.ViewModels.Settings;

[AppLifetime]
[NavigationMetaData(
	Title = "Lurking Wife Mode",
	Searchable = false,
	NavBarPosition = NavBarPosition.Bottom,
	NavBarSelectionMode = NavBarSelectionMode.Toggle)]
public partial class LurkingWifeModeViewModel : RoutableViewModel
{
	[AutoNotify] private bool _lurkingWifeMode;
	[AutoNotify] private string? _iconName;
	[AutoNotify] private string? _iconNameFocused;

	public LurkingWifeModeViewModel(UiContext uiContext, ApplicationSettings applicationSettings) : base(uiContext)
	{
		_lurkingWifeMode = applicationSettings.PrivacyMode;

		this.WhenAnyValue(x => x.LurkingWifeMode)
			.Subscribe(enabled =>
			{
				applicationSettings.PrivacyMode = enabled;
				IconName = enabled ? "eye_hide_regular" : "eye_show_regular";
			});

		applicationSettings.WhenAnyValue(x => x.PrivacyMode)
			.BindTo(this, x => x.LurkingWifeMode);
	}

	public void Toggle()
	{
		LurkingWifeMode = !LurkingWifeMode;
	}
}
