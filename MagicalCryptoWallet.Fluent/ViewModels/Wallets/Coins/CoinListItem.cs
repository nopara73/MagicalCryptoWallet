using System.Collections.Generic;
using System.Reactive.Disposables;
using System.Reactive.Linq;
using System.Windows.Input;
using NBitcoin;
using ReactiveUI;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Models.Wallets;
using MagicalCryptoWallet.Fluent.TreeDataGrid;
using ScriptType = MagicalCryptoWallet.Fluent.Models.Wallets.ScriptType;

namespace MagicalCryptoWallet.Fluent.ViewModels.Wallets.Coins;

public abstract partial class CoinListItem : ViewModelBase, ITreeDataGridExpanderItem, IDisposable
{
	protected readonly CompositeDisposable _disposables = new();

	[AutoNotify] private bool _isParentSelected;
	[AutoNotify] private bool _isParentPointerOver;
	[AutoNotify] private bool _isControlSelected;

	[AutoNotify] private bool _isControlPointerOver;
	[AutoNotify] private bool _isExpanded;
	[AutoNotify] private bool _isCoinjoining;

	protected CoinListItem(UiContext uiContext) : base(uiContext)
	{
		ClipboardCopyCommand = ReactiveCommand.CreateFromTask<string>(text => UiContext.Clipboard.SetTextAsync(text));

		this.WhenAnyValue(x => x.IsControlPointerOver)
			.Do(x =>
			{
				foreach (var child in Children)
				{
					child.IsParentPointerOver = x;
				}
			})
			.Subscribe();

		this.WhenAnyValue(x => x.IsControlSelected)
			.Do(x =>
			{
				foreach (var child in Children)
				{
					child.IsParentSelected = x;
				}
			})
			.Subscribe();
	}

	/// <summary>
	/// Proxy property to prevent stack overflow due to internal bug in Avalonia where the OneWayToSource Binding
	/// is replaced by a TwoWay one.when
	/// </summary>
	public bool IsPointerOverProxy
	{
		get => IsControlPointerOver;
		set => IsControlPointerOver = value;
	}

	public bool IsSelectedProxy
	{
		get => IsControlSelected;
		set => IsControlSelected = value;
	}

	public ICommand? ClipboardCopyCommand { get; protected set; }
	public string? BtcAddress { get; protected set; }

	public bool IsPrivate => Labels == CoinPocketHelper.PrivateFundsText;

	public bool IsSemiPrivate => Labels == CoinPocketHelper.SemiPrivateFundsText;

	public bool IsNonPrivate => !IsSemiPrivate && !IsPrivate;

	public IReadOnlyCollection<CoinViewModel> Children { get; protected set; } = new List<CoinViewModel>();

	public bool IsConfirmed { get; protected set; }

	public bool IsBanned { get; protected set; }

	public string ConfirmationStatus { get; protected set; } = "";

	public Amount Amount { get; protected set; } = new(Money.Zero);

	public string? BannedUntilUtcToolTip { get; protected set; }

	public int? AnonymityScore { get; protected set; }

	public LabelsArray Labels { get; protected set; } = LabelsArray.Empty;

	public DateTimeOffset? BannedUntilUtc { get; protected set; }

	public bool IsChild { get; set; }

	public bool IsLastChild { get; set; }


	public ScriptType? ScriptType { get; protected set; }

	public virtual bool HasChildren() => Children.Count != 0;

	public void Dispose() => _disposables.Dispose();

	public abstract string Key { get; }
}
