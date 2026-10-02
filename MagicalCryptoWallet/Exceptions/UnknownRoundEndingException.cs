using NBitcoin;
using System.Collections.Immutable;
using MagicalCryptoWallet.Blockchain.TransactionOutputs;

namespace MagicalCryptoWallet.Exceptions;

public class UnknownRoundEndingException : Exception
{
	public UnknownRoundEndingException(ImmutableList<SmartCoin> coins, ImmutableList<Script> outputScripts, Exception exception) :
		base(
			$"Round was not ended properly, reason '{exception.Message}'.",
			innerException: exception)
	{
		Coins = coins;
		OutputScripts = outputScripts;
	}

	public ImmutableList<SmartCoin> Coins { get; }
	public ImmutableList<Script> OutputScripts { get; }
}
