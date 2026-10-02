using NBitcoin;
using MagicalCryptoWallet.Blockchain.BlockFilters;

namespace MagicalCryptoWallet.Blockchain.Keys;

public class BlockchainState
{
	public BlockchainState(Network? network = null, ChainHeight? height = null, ChainHeight? birthHeight = null)
	{
		Network = network ?? Network.Main;
		Height = height ?? ChainHeight.Genesis;
		BirthHeight = birthHeight;
	}

	public Network Network { get; }

	private ChainHeight _height = ChainHeight.Genesis;

	/// <summary>Height of the last processed filter.</summary>
	/// <remarks>
	/// Never precedes the first filter we have, otherwise the wallet would ask for the filter right
	/// after it and that filter cannot be fetched.
	/// </remarks>
	public ChainHeight Height
	{
		get => _height;
		set => _height = ChainHeight.Max(value, FilterCheckpoints.GetMagicalCryptoWalletGenesisFilter(Network).Header.Height);
	}

	public ChainHeight? BirthHeight { get; }
}
