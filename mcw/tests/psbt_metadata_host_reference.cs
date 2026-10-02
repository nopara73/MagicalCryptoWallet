// Synthetic child for the actual mcw host and production managed adapter.
// This is test tooling; it never starts a wallet UI or broadcasts a transaction.
using System;
using System.Collections.Generic;
using System.Diagnostics.CodeAnalysis;
using System.Linq;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Mcw.Psbt;

public static class PsbtMetadataHostReference
{
	private sealed class Store(Transaction transaction) : ITransactionStore
	{
		public bool TryGetTransaction(uint256 hash, [NotNullWhen(true)] out SmartTransaction? transactionResult)
		{
			transactionResult = hash == transaction.GetHash() ? new SmartTransaction(transaction) : null;
			return transactionResult is not null;
		}
	}
	private static void Require(bool condition, string message)
	{
		if (!condition) { throw new InvalidOperationException(message); }
	}
	private static void LegacyMetadata(PSBT packet, KeyManager manager, ITransactionStore store)
	{
		if (manager.MasterFingerprint is { } fingerprint)
		{
			foreach (var script in packet.Inputs.Select(input => input.WitnessUtxo?.ScriptPubKey).Concat(packet.Outputs.Select(output => output.ScriptPubKey)).ToArray())
			{
				if (script is not null && manager.TryGetKeyForScriptPubKey(script, out var key))
				{
					packet.AddKeyPath(key.PubKey, new RootedKeyPath(fingerprint, key.FullKeyPath), script);
				}
			}
		}
		foreach (var input in packet.Inputs)
		{
			if (store.TryGetTransaction(input.PrevOut.Hash, out var transaction)) { input.NonWitnessUtxo = transaction.Transaction; }
		}
	}
	public static int Main()
	{
		using var host = ManagedApplicationHost.Connect();
		try
		{
			foreach (bool large in new[] { false, true }) { CompareMetadata(large); }
			FactoryUsesNativeMetadata(tryToSign: false);
			FactoryUsesNativeMetadata(tryToSign: true);
			Console.Error.WriteLine("MCW_PSBT_HOST_VERIFIED metadata=2 factory=2 signing=retained packets=synthetic");
			return 0;
		}
		catch (Exception error)
		{
			Console.Error.WriteLine(error);
			return 1;
		}
	}
	private static KeyManager Wallet() => KeyManager.CreateNew(
		new Mnemonic("abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"),
		"synthetic-psbt-test", Network.RegTest);
	private static Transaction Parent(HdPubKey key, bool large)
	{
		var parent = Transaction.Create(Network.RegTest);
		parent.Inputs.Add(new TxIn(new OutPoint(new uint256(17), 2)));
		parent.Outputs.Add(new TxOut(Money.Satoshis(1000000), key.P2wpkhScript));
		if (large) { parent.Inputs[0].ScriptSig = new Script(new byte[1100000]); }
		return parent;
	}
	private static void CompareMetadata(bool large)
	{
		var wallet = Wallet();
		var key = wallet.GetNextReceiveKey("synthetic input");
		var change = wallet.GetNextChangeKey();
		var parent = Parent(key, large);
		var store = new Store(parent);
		byte[] secret = new byte[32]; secret[31] = 23;
		using var recipient = new Key(secret);
		var builder = Network.RegTest.CreateTransactionBuilder().AddCoins(new Coin(parent, 0))
			.Send(recipient.PubKey.WitHash.ScriptPubKey, Money.Satoshis(500000))
			.SetChange(change.P2wpkhScript).SendFees(Money.Satoshis(1000));
		var source = builder.BuildPSBT(false);
		source.Settings.IsSmart = false;
		source.Settings.SigningOptions = new SigningOptions(SigHash.All, false);
		byte[] proprietary = { 0xfc, 3, (byte)'m', (byte)'c', (byte)'w', 1, 9 };
		source.Unknown.Add(proprietary, new byte[] { 1, 255 });
		source.Inputs[0].Unknown.Add(proprietary, new byte[] { 2, 254 });
		foreach (var output in source.Outputs) { output.Unknown.Add(proprietary, new byte[] { 3, 253 }); }
		var before = source.ToBytes();
		var legacy = source.Clone();
		LegacyMetadata(legacy, wallet, store);
		var native = McwPsbtMetadata.Enrich(source, wallet, store);
		Require(before.SequenceEqual(source.ToBytes()), "Metadata adapter mutated its source packet.");
		Require(legacy.ToBytes().SequenceEqual(native.ToBytes()), "Managed metadata result differs from retained helpers.");
		Require(native.Network == source.Network && native.Settings.IsSmart == source.Settings.IsSmart, "Packet network/settings changed.");
		Require(native.Settings.SigningOptions.EnforceLowR == source.Settings.SigningOptions.EnforceLowR, "Signing options changed.");
		Require(native.GetFee() == legacy.GetFee() && native.GetFee() == Money.Satoshis(1000), "Metadata changed the fee.");
		Require(native.GetGlobalTransaction().ToBytes().SequenceEqual(source.GetGlobalTransaction().ToBytes()), "Unsigned transaction changed.");
		Require(native.Inputs[0].NonWitnessUtxo!.ToBytes().SequenceEqual(parent.ToBytes()), "Parent transaction changed.");
		builder.AddKeys(wallet.GetSecrets("synthetic-psbt-test", key.P2wpkhScript).ToArray()).SignPSBT(native);
		native.Finalize();
		var signed = native.ExtractTransaction();
		Require(!builder.Check(signed).Any(), "Retained signer/policy rejected enriched packet.");
		Require(signed.Inputs[0].WitScript.PushCount > 0, "Retained signer produced no witness.");
	}
	private static void FactoryUsesNativeMetadata(bool tryToSign)
	{
		var wallet = Wallet();
		var key = wallet.GetNextReceiveKey("synthetic factory input");
		var parent = Parent(key, false);
		var smart = new SmartTransaction(parent);
		var coin = new SmartCoin(smart, 0, key);
		coin.SetAnonymitySet(50);
		var store = new Store(parent);
		var factory = new TransactionFactory(Network.RegTest, wallet, new CoinsView(new[] { coin }), store, "synthetic-psbt-test");
		byte[] secret = new byte[32]; secret[31] = 24;
		using var recipient = new Key(secret);
		var parameters = new TransactionParameters(new PaymentIntent(recipient, Money.Satoshis(500000)), new FeeRate(2m),
			AllowUnconfirmed: true, AllowDoubleSpend: false, AllowedInputs: null, TryToSign: tryToSign, OverrideFeeOverpaymentProtection: false);
		var result = factory.BuildTransaction(parameters, () => new LockTime(42));
		Require(result.Signed == tryToSign, "Factory signing mode changed.");
		Require(result.Fee == Money.Satoshis(1000000) - result.Transaction.Transaction.TotalOut, "Factory fee/amount mismatch.");
		Require(result.Transaction.Transaction.LockTime == new LockTime(42), "Factory locktime changed.");
		if (!tryToSign)
		{
			Require(result.Psbt.Inputs[0].HDKeyPaths.ContainsKey(key.PubKey), "Production factory did not receive key origin metadata.");
			Require(result.Psbt.Inputs[0].NonWitnessUtxo!.GetHash() == parent.GetHash(), "Production factory did not receive parent metadata.");
		}
		else
		{
			var verifier = Network.RegTest.CreateTransactionBuilder().AddCoins(new Coin(parent, 0));
			Require(!verifier.Check(result.Transaction.Transaction).Any(), "Production factory's retained signing/policy path failed.");
		}
	}
}
