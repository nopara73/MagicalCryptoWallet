using System.Reactive.Disposables.Fluent;
using System.Reactive.Disposables;
using NBitcoin;
using ReactiveUI;
using System.Reactive.Linq;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Fluent.Helpers;
using MagicalCryptoWallet.Fluent.Infrastructure;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

[AppLifetime]
public partial class WalletSettingsModel : ReactiveObject, IDisposable
{
	private readonly CompositeDisposable _lifetime = new();
	private readonly IServices _services;
	private readonly KeyManager _keyManager;
	private bool _isDirty;

	[AutoNotify] private Money _plebStopThreshold;
	[AutoNotify] private ScriptType _defaultReceiveScriptType;
	[AutoNotify] private PreferredScriptPubKeyType _changeScriptPubKeyType;

	public WalletSettingsModel(IServices services, KeyManager keyManager)
	{
		_services = services;
		_keyManager = keyManager;


		_plebStopThreshold = _keyManager.PlebStopThreshold ?? KeyManager.DefaultPlebStopThreshold;

		_defaultReceiveScriptType = ScriptType.FromEnum(_keyManager.DefaultReceiveScriptType);
		_changeScriptPubKeyType = _keyManager.ChangeScriptPubKeyType;


		this.WhenAnyValue(
				x => x.PlebStopThreshold)
			.Skip(1)
			.Do(_ => SetValues())
			.Subscribe().DisposeWith(_lifetime);

		this.WhenAnyValue(
				x => x.DefaultReceiveScriptType,
				x => x.ChangeScriptPubKeyType)
			.Do(_ => SetValues())
			.Subscribe().DisposeWith(_lifetime);
	}


	public int AnonScoreTarget => Constants.AnonymityScoreTarget;

	public int MinGapLimit => _keyManager.MinGapLimit;


	public void Save()
	{
		// Settings only update an already configured file.
		if (_isDirty) { _keyManager.ToFile(); _isDirty = false; }
	}

	private void SetValues()
	{
		_keyManager.PlebStopThreshold = PlebStopThreshold;
		_keyManager.DefaultReceiveScriptType = ScriptType.ToScriptPubKeyType(DefaultReceiveScriptType);
		_keyManager.ChangeScriptPubKeyType = ChangeScriptPubKeyType;
		_isDirty = true;
	}

	public void RescanWallet(uint startingHeight, int minGapLimit)
	{
		_keyManager.SetResyncParameters(startingHeight + Constants.ResyncHeightMargin, minGapLimit);
	}
	public void Dispose() { _lifetime.Dispose();  }

}
