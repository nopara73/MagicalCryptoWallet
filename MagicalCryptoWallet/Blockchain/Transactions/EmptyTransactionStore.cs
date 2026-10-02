using System.Diagnostics.CodeAnalysis;
using NBitcoin;

namespace MagicalCryptoWallet.Blockchain.Transactions;

public class EmptyTransactionStore : ITransactionStore
{
	public EmptyTransactionStore(Network network)
	{
		Network = network;
	}

	public Network Network { get; }

	public bool TryGetTransaction(uint256 hash, [NotNullWhen(true)] out SmartTransaction? sameStx)
	{
		// Fee estimation has no parent transactions. Reporting a synthetic empty
		// transaction as available causes metadata serialization to fail.
		sameStx = null;
		return false;
	}
}
