using System.Collections.Generic;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent.Helpers;

public static class WalletHelpers
{
	public static WalletType GetType(KeyManager keyManager)
	{
		if (keyManager.Icon is { } icon &&
			Enum.TryParse(typeof(WalletType), icon, true, out var type) &&
			type is { })
		{
			return (WalletType)type;
		}

		return keyManager.IsHardwareWallet ? WalletType.Hardware : WalletType.Normal;
	}

}
