using System.Reactive;
using System.Reactive.Disposables;
using System.Reactive.Disposables.Fluent;
using System.Threading.Tasks;
using System.Windows.Input;
using ReactiveUI;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Fluent.Models.UI;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.ViewModels.Dialogs;
using AddressFunc = System.Func<MagicalCryptoWallet.Fluent.Models.Wallets.IAddress, System.Threading.Tasks.Task>;
using AddressAction = System.Action<MagicalCryptoWallet.Fluent.Models.Wallets.IAddress>;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Receive;

public partial class AddressViewModel : ViewModelBase, IDisposable
{
	private readonly CompositeDisposable _disposables = new();

	[AutoNotify] private string _addressText;
	[AutoNotify] private ScriptType _scriptType;
	[AutoNotify] private LabelsArray _labels;

	public AddressViewModel(UiContext uiContext, AddressFunc onEdit, AddressAction onShow, IAddress address) : base(uiContext)
	{
		Address = address;
		_addressText = address.ShortenedText;

		_scriptType = address.ScriptType;

		this.WhenAnyValue(x => x.Address.Labels)
			.BindTo(this, viewModel => viewModel.Labels)
			.DisposeWith(_disposables);

		CopyAddressCommand = ReactiveCommand.CreateFromTask(() => UiContext.Clipboard.SetTextAsync(Address.Text));
		HideAddressCommand = ReactiveCommand.CreateFromTask(PromptHideAddressAsync);
		EditLabelCommand = ReactiveCommand.CreateFromTask(() => onEdit(address));
		NavigateCommand = ReactiveCommand.Create(() => onShow(address));
	}

	private IAddress Address { get; }

	public ICommand CopyAddressCommand { get; }

	public ICommand HideAddressCommand { get; }

	public ICommand EditLabelCommand { get; }

	public ReactiveCommand<Unit, Unit> NavigateCommand { get; }

	private async Task PromptHideAddressAsync()
	{
		var result = await UiContext.Navigate().NavigateDialogAsync(new ConfirmHideAddressViewModel(UiContext, Address.Labels));

		if (result.Result == false)
		{
			return;
		}

		Address.Hide();

		var isAddressCopied = await UiContext.Clipboard.GetTextAsync() == Address.Text;

		if (isAddressCopied)
		{
			await UiContext.Clipboard.ClearAsync();
		}
	}

	public void Dispose()
	{
		_disposables.Dispose();
	}
}
