using NBitcoin.Secp256k1;
using NBitcoin.WalletPolicies;
using System.Data;
using System.Diagnostics.CodeAnalysis;
using System.IO;
using System.Security;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.BlockFilters;
using MagicalCryptoWallet.CoinJoinProfiles;
using MagicalCryptoWallet.Io;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Serialization;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.Wallets.Slip39;
using Decode = MagicalCryptoWallet.Serialization.Decode;
using Encode = MagicalCryptoWallet.Serialization.Encode;
using Network = NBitcoin.Network;
using OutPoint = NBitcoin.OutPoint;

namespace MagicalCryptoWallet.Blockchain.Keys;

public class KeyManager
{
	public const bool DefaultAutoCoinjoin = false;

	public const int AbsoluteMinGapLimit = 21;
	public const int MaxGapLimit = 10_000;
	public static readonly Money DefaultPlebStopThreshold = Money.Coins(0.005m);

	internal KeyManager(
		BitcoinEncryptedSecretNoEC encryptedSecret,
		byte[] chainCode,
		HDFingerprint? masterFingerprint,
		ExtPubKey extPubKey,
		ExtPubKey? taprootExtPubKey,
		int? minGapLimit,
		BlockchainState blockchainState,
		string? filePath = null,
		KeyPath? segwitAccountKeyPath = null,
		KeyPath? taprootAccountKeyPath = null)
	{
		ArgumentNullException.ThrowIfNull(encryptedSecret);
		ArgumentNullException.ThrowIfNull(chainCode);
		if (chainCode.Length != 32)
		{
			throw new InvalidDataException("The wallet chain code must contain exactly 32 bytes.");
		}
		EncryptedSecret = encryptedSecret;
		ChainCode = chainCode;
		MasterFingerprint = masterFingerprint;
		SegwitExtPubKey = extPubKey;
		TaprootExtPubKey = taprootExtPubKey;

		MinGapLimit = Math.Max(AbsoluteMinGapLimit, minGapLimit ?? 0);

		_blockchainState = blockchainState;

		SegwitAccountKeyPath = segwitAccountKeyPath ?? GetAccountKeyPath(_blockchainState.Network, ScriptPubKeyType.Segwit);
		SegwitExternalKeyGenerator = new HdPubKeyGenerator(SegwitExtPubKey.Derive(0), SegwitAccountKeyPath.Derive(0), MinGapLimit);
		_segwitInternalKeyGenerator = new HdPubKeyGenerator(SegwitExtPubKey.Derive(1), SegwitAccountKeyPath.Derive(1), MinGapLimit);

		TaprootAccountKeyPath = taprootAccountKeyPath ?? GetAccountKeyPath(_blockchainState.Network, ScriptPubKeyType.TaprootBIP86);
		if (TaprootExtPubKey is { })
		{
			TaprootExternalKeyGenerator = new HdPubKeyGenerator(TaprootExtPubKey.Derive(0), TaprootAccountKeyPath.Derive(0), MinGapLimit);
			_taprootInternalKeyGenerator = new HdPubKeyGenerator(TaprootExtPubKey.Derive(1), TaprootAccountKeyPath.Derive(1), MinGapLimit);
		}

		SetFilePath(filePath);

		ToFile();
	}

	public static KeyPath GetAccountKeyPath(Network network, ScriptPubKeyType scriptPubKeyType) =>
		new((network.Name, scriptPubKeyType) switch
		{
			("TestNet4", ScriptPubKeyType.Segwit) => "m/84h/1h/0h",
			("signet", ScriptPubKeyType.Segwit) => "m/84h/1h/0h",
			("RegTest", ScriptPubKeyType.Segwit) => "m/84h/0h/0h",
			("Main", ScriptPubKeyType.Segwit) => "m/84h/0h/0h",
			("TestNet4", ScriptPubKeyType.TaprootBIP86) => "m/86h/1h/0h",
			("signet", ScriptPubKeyType.TaprootBIP86) => "m/86h/1h/0h",
			("RegTest", ScriptPubKeyType.TaprootBIP86) => "m/86h/0h/0h",
			("Main", ScriptPubKeyType.TaprootBIP86) => "m/86h/0h/0h",
			_ => throw new ArgumentException($"Unknown account for network '{network}' and script type {scriptPubKeyType}.")
		});

	public WalletPolicy GetWpkhWalletPolicy(string password, Network network)
	{
		if (!MasterFingerprint.HasValue)
		{
			throw new InvalidOperationException($"{nameof(MasterFingerprint)} is not defined.");
		}

		return WpkhWalletPolicyHelper.Get(network, MasterFingerprint.Value, GetMasterExtKey(password), SegwitAccountKeyPath);
	}

	#region Properties

	public BitcoinEncryptedSecretNoEC EncryptedSecret { get; }

	public byte[] ChainCode { get; }

	public HDFingerprint? MasterFingerprint { get; private set; }

	public ExtPubKey SegwitExtPubKey { get; }

	public ExtPubKey? TaprootExtPubKey { get; private set; }

	public int MinGapLimit { get; private set; }

	public KeyPath SegwitAccountKeyPath { get; private set; }

	public KeyPath TaprootAccountKeyPath { get; private set; }

	private readonly BlockchainState _blockchainState;

	public bool AutoCoinJoin { get; set; } = DefaultAutoCoinjoin;

	/// <summary>
	/// Won't coinjoin automatically if the confirmed wallet balance is below this.
	/// </summary>
	public Money PlebStopThreshold { get; set; } = DefaultPlebStopThreshold;

	public int AnonScoreTarget { get; set; } = PrivacyProfiles.DefaultProfile.AnonScoreTarget;

	public bool NonPrivateCoinIsolation { get; set; } = PrivacyProfiles.DefaultProfile.NonPrivateCoinIsolation;

	public bool OnlyUsePrivateFundsForPayments { get; set; } = PrivacyProfiles.DefaultProfile.OnlyUsePrivateFundsForPayments;

	public ScriptPubKeyType DefaultReceiveScriptType { get; set; } = ScriptPubKeyType.TaprootBIP86;

	public PreferredScriptPubKeyType ChangeScriptPubKeyType { get; set; } = PreferredScriptPubKeyType.Unspecified.Instance;

	public Dictionary<uint256, CoinjoinCosts> CoinjoinCosts { get; private set; } = new();

	public string? FilePath { get; private set; }

	public IEnumerable<ScriptPubKeyType> AvailableScriptPubKeyTypes => TaprootExtPubKey is null
		? [ScriptPubKeyType.Segwit]
		: [ScriptPubKeyType.Segwit, ScriptPubKeyType.TaprootBIP86];

	private readonly HdPubKeyCache _hdPubKeyCache = new();

	// `_criticalStateLock` is aimed to synchronize read/write access to the "critical" properties:
	// keys (stored in the `_hdPubKeyCache`), minGapLimit, secrets, height, network.
	private readonly Lock _criticalStateLock = new();

	#endregion Properties

	private HdPubKeyGenerator SegwitExternalKeyGenerator { get; set; }
	private readonly HdPubKeyGenerator _segwitInternalKeyGenerator;
	private HdPubKeyGenerator? TaprootExternalKeyGenerator { get; set; }
	private readonly HdPubKeyGenerator? _taprootInternalKeyGenerator;

	public static KeyManager CreateNew(out Mnemonic mnemonic, string password, Network network, string? filePath = null)
	{
		mnemonic = new Mnemonic(Wordlist.English, WordCount.Twelve);
		return CreateNew(mnemonic, password, network, filePath);
	}

	public static KeyManager CreateNew(Mnemonic mnemonic, string password, Network network, string? filePath = null)
	{
		password ??= "";
		var seed = mnemonic.DeriveSeed(password);
		return CreateNew(seed, password, network, filePath);
	}

	public static KeyManager CreateNew(Share[] shares, string password, Network network, string? filePath = null)
	{
		password ??= "";
		var seed = Shamir.Combine(shares, password);
		return CreateNew(seed, password, network, filePath);
	}

	private static KeyManager CreateNew(byte[] seed, string password, Network network, string? filePath = null)
	{
		var extKey = ExtKey.CreateFromSeed(seed);
		var encryptedSecret = extKey.PrivateKey.GetEncryptedBitcoinSecret(password, network);

		HDFingerprint masterFingerprint = extKey.Neuter().PubKey.GetHDFingerPrint();
		var birthHeight = FilterCheckpoints.GetMostRecentCheckpoint(network).Header.Height;
		BlockchainState blockchainState = new(network, birthHeight: birthHeight);
		KeyPath segwitAccountKeyPath = GetAccountKeyPath(network, ScriptPubKeyType.Segwit);
		ExtPubKey segwitExtPubKey = extKey.Derive(segwitAccountKeyPath).Neuter();

		KeyPath taprootAccountKeyPath = GetAccountKeyPath(network, ScriptPubKeyType.TaprootBIP86);
		ExtPubKey taprootExtPubKey = extKey.Derive(taprootAccountKeyPath).Neuter();

		return new KeyManager(encryptedSecret, extKey.ChainCode, masterFingerprint, segwitExtPubKey, taprootExtPubKey, AbsoluteMinGapLimit, blockchainState, filePath, segwitAccountKeyPath, taprootAccountKeyPath);
	}

	public static KeyManager Recover(Mnemonic mnemonic, string password, Network network, KeyPath swAccountKeyPath, KeyPath? trAccountKeyPath = null, string? filePath = null, int minGapLimit = AbsoluteMinGapLimit, ChainHeight? birthHeight = null)
	{
		password ??= "";
		var seed = mnemonic.DeriveSeed(password);
		return Recover(seed, password, network, swAccountKeyPath, trAccountKeyPath, filePath, minGapLimit, birthHeight);
	}

	public static KeyManager Recover(Share[] shares, string password, Network network, KeyPath swAccountKeyPath, KeyPath? trAccountKeyPath = null, string? filePath = null, int minGapLimit = AbsoluteMinGapLimit, ChainHeight? birthHeight = null)
	{
		password ??= "";
		var seed = Shamir.Combine(shares, password);
		return Recover(seed, password, network, swAccountKeyPath, trAccountKeyPath, filePath, minGapLimit, birthHeight);
	}

	private static KeyManager Recover(byte[] seed, string password, Network network, KeyPath swAccountKeyPath, KeyPath? trAccountKeyPath = null, string? filePath = null, int minGapLimit = AbsoluteMinGapLimit, ChainHeight? birthHeight = null)
	{
		ExtKey extKey = ExtKey.CreateFromSeed(seed);
		var encryptedSecret = extKey.PrivateKey.GetEncryptedBitcoinSecret(password, network);

		HDFingerprint masterFingerprint = extKey.Neuter().PubKey.GetHDFingerPrint();

		KeyPath segwitAccountKeyPath = swAccountKeyPath ?? GetAccountKeyPath(network, ScriptPubKeyType.Segwit);
		ExtPubKey segwitExtPubKey = extKey.Derive(segwitAccountKeyPath).Neuter();
		KeyPath taprootAccountKeyPath = trAccountKeyPath ?? GetAccountKeyPath(network, ScriptPubKeyType.TaprootBIP86);
		ExtPubKey taprootExtPubKey = extKey.Derive(taprootAccountKeyPath).Neuter();

		birthHeight ??= FilterCheckpoints.GetMagicalCryptoWalletGenesisFilter(network).Header.Height;
		var blockchainState = new BlockchainState(network, height: birthHeight, birthHeight: birthHeight);
		var km = new KeyManager(encryptedSecret, extKey.ChainCode, masterFingerprint, segwitExtPubKey, taprootExtPubKey, minGapLimit, blockchainState, filePath, segwitAccountKeyPath, taprootAccountKeyPath);
		km.AssertCleanKeysIndexedNoLock();
		return km;
	}

	public static KeyManager FromFile(string filePath)
	{
		if (!File.Exists(filePath))
		{
			throw new FileNotFoundException($"Wallet file not found at: `{filePath}`.");
		}

		string jsonString = File.SafelyReadAllText(filePath, Encoding.UTF8);

		KeyManager km = JsonDecoder.FromString(jsonString, Decoder)
			?? throw new DataException($"Wallet file at: `{filePath}` is not a valid wallet file or it is corrupted.");

		km.SetFilePath(filePath);

		return km;
	}

	public void SetFilePath(string? filePath)
	{
		FilePath = string.IsNullOrWhiteSpace(filePath) ? null : filePath;
		if (FilePath is null)
		{
			return;
		}

		IoHelpers.EnsureContainingDirectoryExists(FilePath);
	}

	internal HdPubKey GenerateNewKey(LabelsArray labels, KeyState keyState, bool isInternal, ScriptPubKeyType scriptPubKeyType = ScriptPubKeyType.Segwit)
	{
		var hdPubKeyRegistry = GetHdPubKeyGenerator(isInternal, scriptPubKeyType)
			?? throw new NotSupportedException($"Script type '{scriptPubKeyType}' is not supported.");

		lock (_criticalStateLock)
		{
			var view = _hdPubKeyCache.GetView(hdPubKeyRegistry.KeyPath);
			var (keyPath, extPubKey) = hdPubKeyRegistry.GenerateNewKey(view);
			var hdPubKey = new HdPubKey(extPubKey.PubKey, keyPath, labels, keyState);
			_hdPubKeyCache.AddKey(hdPubKey, scriptPubKeyType);
			return hdPubKey;
		}
	}

	public HdPubKey GetNextReceiveKey(LabelsArray labels, ScriptPubKeyType scriptPubKeyType = ScriptPubKeyType.Segwit)
	{
		lock (_criticalStateLock)
		{
			var (generator, generatorSetter) = scriptPubKeyType switch
			{
				ScriptPubKeyType.Segwit => (SegwitExternalKeyGenerator, (Action<HdPubKeyGenerator>)(g => SegwitExternalKeyGenerator = g)),
				ScriptPubKeyType.TaprootBIP86 => (TaprootExternalKeyGenerator, (g => TaprootExternalKeyGenerator = g)),
				_ => throw new NotSupportedException($"Script type '{scriptPubKeyType}' is not supported.")
			};

			if (generator is not { } nonNullKeyGenerator)
			{
				throw new NotSupportedException("Taproot is not supported in this wallet.");
			}

			var (newKey, newlyGeneratedKeySet, newHdPubKeyGenerator) = GetNextReceiveKey(nonNullKeyGenerator);
			generatorSetter(newHdPubKeyGenerator);
			_hdPubKeyCache.AddRangeKeys(newlyGeneratedKeySet);
			newKey.SetLabel(labels);
			ToFileNoLock();
			return newKey;
		}
	}

	private (HdPubKey, HdPubKey[], HdPubKeyGenerator) GetNextReceiveKey(HdPubKeyGenerator hdPubKeyGenerator)
	{
		// Find the next clean external key with an empty label.
		HdPubKeyPathView externalView;

		lock (_criticalStateLock)
		{
			externalView = _hdPubKeyCache.GetView(hdPubKeyGenerator.KeyPath);
		}

		if (externalView.CleanKeys.FirstOrDefault(x => x.Labels.IsEmpty) is { } cachedKey)
		{
			return (cachedKey, [], hdPubKeyGenerator);
		}

		var newHdPubKeyGenerator = hdPubKeyGenerator with { MinGapLimit = hdPubKeyGenerator.MinGapLimit + 1 };
		var newHdPubKeys = newHdPubKeyGenerator.AssertCleanKeysIndexed(externalView).Select(CreateHdPubKey).ToArray();

		var newKey = newHdPubKeys.First();
		return (newKey, newHdPubKeys, newHdPubKeyGenerator);
	}

	public HdPubKey GetNextChangeKey() =>
		GetKeys(x =>
			x.KeyState == KeyState.Clean &&
			x.IsInternal &&
			MatchesChangeScriptPubKeyType(x))
			.First();

	public ImmutableArray<HdPubKey> GetNextCoinJoinKeys() =>
		GetKeys(x =>
				x.KeyState == KeyState.Locked &&
				x.IsInternal == true);

	private bool MatchesChangeScriptPubKeyType(HdPubKey hd) =>
		ChangeScriptPubKeyType switch
		{
			PreferredScriptPubKeyType.Unspecified => true,
			PreferredScriptPubKeyType.Specified scriptType => hd.FullKeyPath.GetScriptTypeFromKeyPath() == scriptType.ScriptType,
			_ => throw new ArgumentOutOfRangeException()
		};

	public ImmutableArray<HdPubKey> GetKeys(Func<HdPubKey, bool>? wherePredicate)
	{
		// BIP44-ish derivation scheme
		// m / purpose' / coin_type' / account' / change / address_index
		lock (_criticalStateLock)
		{
			AssertCleanKeysIndexedNoLock();
			var predicate = wherePredicate ?? (_ => true);
			return _hdPubKeyCache.HdPubKeys.Where(predicate).OrderBy(x => x.Index).ToImmutableArray();
		}
	}

	public ImmutableArray<HdPubKey> GetKeys(KeyState? keyState = null, bool? isInternal = null) =>
		(keyState, isInternal) switch
		{
			(null, null) => GetKeys(x => true),
			(null, { } i) => GetKeys(x => x.IsInternal == i),
			({ } k, null) => GetKeys(x => x.KeyState == k),
			({ } k, { } i) => GetKeys(x => x.IsInternal == i && x.KeyState == k)
		};

	/// <summary>
	/// This function can only be called for wallet synchronization.
	/// It's unsafe because it doesn't assert that the GapLimit is respected.
	/// GapLimit should be enforced whenever a transaction is discovered.
	/// </summary>
	public byte[][] UnsafeGetSynchronizationInfos()
	{
		lock (_criticalStateLock)
		{
			// A copy must be returned because the cache may change.
			return _hdPubKeyCache.Select(x => x.ScriptPubKeyBytes).ToArray();
		}
	}

	public bool TryGetKeyForScriptPubKey(Script scriptPubKey, [NotNullWhen(true)] out HdPubKey? hdPubKey)
	{
		lock (_criticalStateLock)
		{
			return _hdPubKeyCache.TryGetPubKey(scriptPubKey, out hdPubKey);
		}
	}

	public IEnumerable<Key> GetSecrets(string password, params Script[] scripts)
	{
		ExtKey extKey = GetMasterExtKey(password);

		foreach (HdPubKey key in GetKeys(x => scripts.Contains(x.P2wpkhScript) || scripts.Contains(x.P2Taproot)))
		{
			yield return extKey.Derive(key.FullKeyPath).PrivateKey;
		}
	}

	public ExtKey GetMasterExtKey(string password)
	{
		password ??= "";

		try
		{
			Key secret = EncryptedSecret.GetKey(password);
			var extKey = new ExtKey(secret, ChainCode);

			// Backwards compatibility:
			MasterFingerprint ??= secret.PubKey.GetHDFingerPrint();
			DeriveTaprootExtPubKey(extKey);

			return extKey;
		}
		catch (SecurityException ex)
		{
			throw new SecurityException("Invalid password.", ex);
		}
	}

	private void DeriveTaprootExtPubKey(ExtKey extKey)
	{
		if (TaprootExtPubKey is null)
		{
			TaprootAccountKeyPath = GetAccountKeyPath(GetNetwork(), ScriptPubKeyType.TaprootBIP86);
			TaprootExtPubKey = extKey.Derive(TaprootAccountKeyPath).Neuter();
		}
	}

	public void SetKeyState(KeyState newKeyState, HdPubKey hdPubKey)
	{
		if (hdPubKey.KeyState == newKeyState)
		{
			return;
		}

		hdPubKey.SetKeyState(newKeyState);
		if (newKeyState is KeyState.Locked or KeyState.Used)
		{
			var keySource = GetHdPubKeyGenerator(hdPubKey.IsInternal, hdPubKey.FullKeyPath.GetScriptTypeFromKeyPath());

			// This can happen after downgrading to pre-taproot magicalcryptowallet version the switching back to a supporting
			// version so taproot keys are detected. However, the user has not login yet so taprootextpubkey is
			// not derived yet (because pre-taproot magicalcryptowallet do not serialize fields that it doesn't know)
			if (keySource is not null)
			{
				lock (_criticalStateLock)
				{
					var view = _hdPubKeyCache.GetView(keySource.KeyPath);
					var keys = keySource.AssertCleanKeysIndexed(view).Select(CreateHdPubKey);
					_hdPubKeyCache.AddRangeKeys(keys);
				}
			}
		}
	}

	private HdPubKeyGenerator? GetHdPubKeyGenerator(bool isInternal, ScriptPubKeyType scriptPubKeyType) =>
		(isInternal, scriptPubKeyType) switch
		{
			(true, ScriptPubKeyType.Segwit) => _segwitInternalKeyGenerator,
			(false, ScriptPubKeyType.Segwit) => SegwitExternalKeyGenerator,
			(true, ScriptPubKeyType.TaprootBIP86) => _taprootInternalKeyGenerator,
			(false, ScriptPubKeyType.TaprootBIP86) => TaprootExternalKeyGenerator,
			_ => throw new NotSupportedException($"There is not available generator for '{scriptPubKeyType}.")
		};

	private void AssertCleanKeysIndexedNoLock()
	{
		var keys = new[]
			{
				_segwitInternalKeyGenerator,
				SegwitExternalKeyGenerator,
				_taprootInternalKeyGenerator,
				TaprootExternalKeyGenerator
			}
			.Where(x => x is not null)
			.SelectMany(gen => gen!.AssertCleanKeysIndexed(_hdPubKeyCache.GetView(gen.KeyPath)))
			.Select(CreateHdPubKey);

		_hdPubKeyCache.AddRangeKeys(keys);
	}

	/// <summary>
	/// Make sure there's always locked internal keys generated and indexed.
	/// </summary>
	public void AssertLockedInternalKeysIndexedAndPersist(int howMany, bool preferTaproot)
	{
		lock (_criticalStateLock)
		{
			if (AssertLockedInternalKeysIndexedNoLock(howMany, preferTaproot))
			{
				ToFileNoLock();
			}
		}
	}

	private bool AssertLockedInternalKeysIndexedNoLock(int howMany, bool preferTaproot)
	{
		var hdPubKeyGenerator = (_taprootInternalKeyGenerator, preferTaproot) switch
		{
			({ }, true) => _taprootInternalKeyGenerator,
			_ => _segwitInternalKeyGenerator
		};

		Guard.InRangeAndNotNull(nameof(howMany), howMany, 0, hdPubKeyGenerator.MinGapLimit);
		var internalView = _hdPubKeyCache.GetView(hdPubKeyGenerator.KeyPath);
		var lockedKeyCount = internalView.LockedKeys.Count();
		var missingLockedKeys = Math.Max(howMany - lockedKeyCount, 0);

		_hdPubKeyCache.AddRangeKeys(hdPubKeyGenerator.AssertCleanKeysIndexed(internalView).Select(CreateHdPubKey));

		var availableCandidates = _hdPubKeyCache
			.GetView(hdPubKeyGenerator.KeyPath)
			.CleanKeys
			.Where(x => x.Labels.IsEmpty)
			.Take(missingLockedKeys)
			.ToList();

		foreach (var hdPubKeys in availableCandidates)
		{
			SetKeyState(KeyState.Locked, hdPubKeys);
		}

		return availableCandidates.Count > 0;
	}

	public void ToFile()
	{
		lock (_criticalStateLock)
		{
			ToFileNoLock();
		}
	}

	private void ToFileNoLock()
	{
		if (FilePath is not { } filePath)
		{
			return;
		}

		string jsonString = JsonEncoder.ToReadableString(this, EncodeKeyManagerNoLock);
		File.SafelyWriteAllText(filePath, jsonString, Encoding.UTF8);
	}

	#region _blockchainState

	public ChainHeight GetBestHeight()
	{
		lock (_criticalStateLock)
		{
			return _blockchainState.Height;
		}
	}

	public Network GetNetwork()
	{
		return _blockchainState.Network;
	}

	public ChainHeight? GetBirthHeight()
	{
		lock (_criticalStateLock)
		{
			return _blockchainState.BirthHeight;
		}
	}

	public void SetBestHeight(ChainHeight height, bool toFile = true)
	{
		lock (_criticalStateLock)
		{
			_blockchainState.Height = height;
			if (toFile)
			{
				ToFileNoLock();
			}
		}
	}

	public void SetResyncParameters(ChainHeight newStartingHeight, int newMinGapLimit)
	{
		lock (_criticalStateLock)
		{
			_blockchainState.Height = newStartingHeight;
			MinGapLimit = newMinGapLimit;
			ToFileNoLock();
		}
	}

	public void SetMaxBestHeight(ChainHeight newHeight)
	{
		lock (_criticalStateLock)
		{
			var prevHeight = _blockchainState.Height;
			if (newHeight < prevHeight)
			{
				SetBestHeight(newHeight);
				Logger.LogWarning($"Wallet height has been set back by {prevHeight - newHeight}. From {prevHeight} to {newHeight}.");
			}
		}
	}

	#endregion _blockchainState

	private static HdPubKey CreateHdPubKey((KeyPath KeyPath, ExtPubKey ExtPubKey) x) =>
		new(x.ExtPubKey.PubKey, x.KeyPath, LabelsArray.Empty, KeyState.Clean);

	public void AddCoinjoinCosts(uint256 transactionId, CoinjoinCosts coinjoinCosts)
	{
		CoinjoinCosts[transactionId] = coinjoinCosts;
		ToFile();
	}

	private static JsonNode EncodeKeyManagerNoLock(KeyManager keyManager) =>
		Encode.Object([
			("EncryptedSecret", Encode.BitcoinEncryptedSecretNoEC(keyManager.EncryptedSecret)),
			("ChainCode", Encode.ChainCode(keyManager.ChainCode)),
			("MasterFingerprint", Encode.Optional(keyManager.MasterFingerprint, Encode.HDFingerprint)),
			("ExtPubKey", Encode.ExtPubKey(keyManager.SegwitExtPubKey)),
			("TaprootExtPubKey", Encode.Optional(keyManager.TaprootExtPubKey, Encode.ExtPubKey)),
			("MinGapLimit", Encode.Int(keyManager.MinGapLimit)),
			("AccountKeyPath", Encode.KeyPath(keyManager.SegwitAccountKeyPath)),
			("TaprootAccountKeyPath", Encode.KeyPath(keyManager.TaprootAccountKeyPath)),
			("BlockchainState", Encode.BlockchainState(keyManager._blockchainState)),
			("AutoCoinJoin", Encode.Bool(keyManager.AutoCoinJoin)),
			("PlebStopThreshold", Encode.MoneyBitcoins(keyManager.PlebStopThreshold)),
			("AnonScoreTarget", Encode.Int(keyManager.AnonScoreTarget)),
			("RedCoinIsolation", Encode.Bool(keyManager.NonPrivateCoinIsolation)),
			("OnlyUsePrivateFundsForPayments", Encode.Bool(keyManager.OnlyUsePrivateFundsForPayments)),
			("DefaultReceiveScriptType", Encode.ScriptPubKeyType(keyManager.DefaultReceiveScriptType)),
			("ChangeScriptPubKeyType", Encode.PreferredScriptPubKeyType(keyManager.ChangeScriptPubKeyType)),
			("CoinjoinCosts", Encode.Array(keyManager.CoinjoinCosts.Select(Encode.CoinjoinCosts))),
			("HdPubKeys", Encode.Array(keyManager._hdPubKeyCache.HdPubKeys.Select(Encode.HdPubKey)))
		]);

	private static readonly Decoder<KeyManager> Decoder =
		Decode.Object(get =>
		{
			if (!get.Value.TryGetProperty("EncryptedSecret", out var encryptedSecret) || encryptedSecret.ValueKind == JsonValueKind.Null)
			{
				throw new NotSupportedException("This wallet has no local private keys. Hardware and watch-only wallets are no longer supported. Open it with a compatible wallet application.");
			}
			var chainCode = new byte[32];
			if (!get.Value.TryGetProperty("ChainCode", out var encodedChainCode)
				|| encodedChainCode.ValueKind != JsonValueKind.String
				|| !Convert.TryFromBase64String(encodedChainCode.GetString()!, chainCode, out var bytesWritten)
				|| bytesWritten != chainCode.Length)
			{
				throw new InvalidDataException("The wallet chain code must contain exactly 32 bytes.");
			}
			var fingerprint = Decode.Field("MasterFingerprint", Decode.HDFingerprint)(get.Value).Match(v => v, _ => (HDFingerprint?)null);

			// todo: review again
			var blockchainState = get.Required("BlockchainState", Decode.BlockchainState);
			var network = blockchainState.Network;
			var encryptedSecretDecoder = Decode.String.Map(s => new BitcoinEncryptedSecretNoEC(s, network));
			var km = new KeyManager(
				get.Required("EncryptedSecret", encryptedSecretDecoder),
				chainCode,
				fingerprint,

				get.Required("ExtPubKey", Decode.ExtPubKey),
				get.Optional("TaprootExtPubKey", Decode.ExtPubKey),
				get.Optional("MinGapLimit", Decode.Int),
				blockchainState,
				(string?) "",
				get.Optional("AccountKeyPath", Decode.KeyPath),
				get.Optional("TaprootAccountKeyPath", Decode.KeyPath)
			)
			{
				AutoCoinJoin = get.Optional("AutoCoinJoin", Decode.Bool, false),
				PlebStopThreshold = get.Optional("PlebStopThreshold", Decode.MoneyBitcoins) ?? DefaultPlebStopThreshold,
				AnonScoreTarget = get.Optional("AnonScoreTarget", Decode.Int, 10),
				NonPrivateCoinIsolation = get.Optional("RedCoinIsolation", Decode.Bool, false),
				OnlyUsePrivateFundsForPayments = get.Optional("OnlyUsePrivateFundsForPayments", Decode.Bool, false),
				DefaultReceiveScriptType = get.Optional("DefaultReceiveScriptType", Decode.ScriptPubKeyType, ScriptPubKeyType.TaprootBIP86),
				ChangeScriptPubKeyType = get.Optional("ChangeScriptPubKeyType", Decode.PreferredScriptPubKeyType) ?? PreferredScriptPubKeyType.Unspecified.Instance,
				CoinjoinCosts = get.Optional("CoinjoinCosts", Decode.Array(Decode.CoinjoinCosts))?.ToDictionary() ?? []
			};
			km._hdPubKeyCache.AddRangeKeys(get.Required("HdPubKeys", Decode.Array(Decode.HdPubKey)));
			return km;
		});
}

public static class KeyPathExtensions
{
	public static ScriptPubKeyType GetScriptTypeFromKeyPath(this KeyPath keyPath) =>
		keyPath.ToBytes().First() switch
		{
			84 => ScriptPubKeyType.Segwit,
			86 => ScriptPubKeyType.TaprootBIP86,
			_ => ScriptPubKeyType.Segwit // User can specify a specify whatever (like m/999'/999'/999')
										 // throw new NotSupportedException("Unknown script type.")
		};
}

public static class HdPubKeyExtensions
{
	public static BitcoinAddress GetAddress(this HdPubKey me, Network network) =>
		me.PubKey.GetAddress(me.FullKeyPath.GetScriptTypeFromKeyPath(), network);

	public static Script GetAssumedScriptPubKey(this HdPubKey me) =>
		me.PubKey.GetScriptPubKey(me.FullKeyPath.GetScriptTypeFromKeyPath());
}
