using NBitcoin;
using System.Collections.Generic;
using System.Linq;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Userfacing;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.Transactions;

namespace MagicalCryptoWallet.Wallets;

/// <summary>Private keys belong to an explicit authorization lifetime, never to the loaded wallet.</summary>
public sealed class WalletAuthorization : IDisposable
{
	private ExtKey? _masterKey;
	private string? _password;
	private WalletAuthorization(KeyManager keyManager, ExtKey masterKey, string password, bool compatibilityPasswordUsed)
	{
		KeyManager = keyManager;
		_masterKey = masterKey;
		_password = password;
		CompatibilityPasswordUsed = compatibilityPasswordUsed;
	}
	public KeyManager KeyManager { get; }
	public bool CompatibilityPasswordUsed { get; }
	public ExtKey MasterKey => _masterKey ?? throw new ObjectDisposedException(nameof(WalletAuthorization));
	public static WalletAuthorization Create(KeyManager keyManager, string password)
	{
		var key = PasswordHelper.GetMasterExtKey(keyManager, password, out var compatible);
		return new(keyManager, key, compatible ?? password, compatible is not null);
	}
	public WalletAuthorization Retain() => new(KeyManager, MasterKey, _password!, CompatibilityPasswordUsed);
	public IEnumerable<Key> GetSecrets(params Script[] scripts) => KeyManager.GetKeys(x => scripts.Contains(x.P2wpkhScript) || scripts.Contains(x.P2Taproot)).Select(x => MasterKey.Derive(x.FullKeyPath).PrivateKey);
	public BuildTransactionResult Sign(BuildTransactionResult preview)
	{
		var psbt = preview.Psbt.Clone();
		var builder = KeyManager.GetNetwork().CreateTransactionBuilder();
		builder.AddCoins(preview.SpentCoins.Select(x => x.Coin).ToArray());
		builder.AddKeys(GetSecrets(preview.SpentCoins.Select(x => x.ScriptPubKey).ToArray()).ToArray());
		builder.SignPSBT(psbt);
		psbt.Finalize();
		var transaction = new SmartTransaction(psbt.ExtractTransaction(), labels: preview.Transaction.Labels);
		foreach (var coin in preview.SpentCoins) { transaction.TryAddWalletInput(coin); }
		foreach (var coin in preview.InnerWalletOutputs) { transaction.TryAddWalletOutput(coin); }
		if (preview.Transaction.IsSpeedup) { transaction.SetSpeedup(); }
		if (preview.Transaction.IsCancellation) { transaction.SetCancellation(); }
		return new(transaction, psbt, true, preview.Fee, preview.FeePercentOfSent, preview.HdPubKeysWithNewLabels);
	}
	public bool VerifyRecoveryWords(Mnemonic mnemonic)
	{
		ObjectDisposedException.ThrowIf(_password is null, this);
		var recovered = KeyManager.Recover(mnemonic, _password, KeyManager.GetNetwork(), KeyManager.SegwitAccountKeyPath, null, null, KeyManager.MinGapLimit);
		return recovered.SegwitExtPubKey == KeyManager.SegwitExtPubKey;
	}
	public void Dispose() { _masterKey = null; _password = null; }
}
