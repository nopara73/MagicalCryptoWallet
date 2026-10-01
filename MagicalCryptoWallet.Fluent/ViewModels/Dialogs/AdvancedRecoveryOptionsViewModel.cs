using System.Globalization;
using System.Reactive.Linq;
using ReactiveUI;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Fluent.Validation;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs.Base;
using MagicalCryptoWallet.Models;

namespace MagicalCryptoWallet.Fluent.ViewModels.Dialogs;

[NavigationMetaData(Title = "Advanced Recovery Options", NavigationTarget = NavigationTarget.CompactDialogScreen)]
public partial class AdvancedRecoveryOptionsViewModel : DialogViewModelBase<(int MinGapLimit, uint BirthHeight)?>
{
	[AutoNotify] private string _minGapLimit;
	[AutoNotify] private string _birthHeight;

	private readonly uint _magicalcryptowalletGenesisHeight;

	public AdvancedRecoveryOptionsViewModel(UiContext uiContext, int minGapLimit,
		Height.ChainHeight magicalcryptowalletGenesisHeight) : base(uiContext)
	{
		_minGapLimit = minGapLimit.ToString();
		_magicalcryptowalletGenesisHeight = magicalcryptowalletGenesisHeight;
		_birthHeight = magicalcryptowalletGenesisHeight.Height.ToString(CultureInfo.InvariantCulture);

		this.ValidateProperty(x => x.MinGapLimit, ValidateMinGapLimit);
		this.ValidateProperty(x => x.BirthHeight, ValidateBirthHeight);

		SetupCancel(enableCancel: true, enableCancelOnEscape: true, enableCancelOnPressed: true);
		EnableBack = false;

		NextCommand = ReactiveCommand.Create(
			() => Close(result: (int.Parse(MinGapLimit), uint.Parse(BirthHeight))),
			this.WhenAnyValue(x => x.MinGapLimit, x => x.BirthHeight).Select(_ => !Validations.Any));
	}

	private void ValidateMinGapLimit(IValidationErrors errors)
	{
		if (!int.TryParse(MinGapLimit, out var minGapLimit) ||
			minGapLimit is < KeyManager.AbsoluteMinGapLimit or > KeyManager.MaxGapLimit)
		{
			errors.Add(
				ErrorSeverity.Error,
				$"Must be a number between {KeyManager.AbsoluteMinGapLimit} and {KeyManager.MaxGapLimit}.");
		}
	}

	private void ValidateBirthHeight(IValidationErrors errors)
	{
		if (!uint.TryParse(BirthHeight, out var height) || height < _magicalcryptowalletGenesisHeight)
		{
			errors.Add(
				ErrorSeverity.Error,
				$"Must be a number greater than or equal to {_magicalcryptowalletGenesisHeight}.");
		}
	}
}
