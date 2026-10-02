using NBitcoin;

namespace MagicalCryptoWallet.Blockchain.Keys;

public record HdPubKeyInfo(HdPubKey HdPubKey, ScriptPubKeyType ScriptPubKeyType)
{
	public Script ScriptPubKey { get; } = HdPubKey.PubKey.GetScriptPubKey(ScriptPubKeyType);
	public byte[] ScriptPubKeyBytes => ScriptPubKey.ToBytes(@unsafe: true);
}
