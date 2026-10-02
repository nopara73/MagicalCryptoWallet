using NBitcoin;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;

namespace MagicalCryptoWallet.Blockchain.TransactionBuilding;

public sealed record Destination(Script ScriptPubKey)
{
	public static implicit operator Destination(BitcoinAddress address) => new(address.ScriptPubKey);
	public static implicit operator Destination(Script scriptPubKey) => new(scriptPubKey);

	public string ToString(Network network) =>
		ScriptPubKey.GetDestinationAddress(network)?.ToString()
		?? throw new ArgumentException("Unknown destination script");
}

public class DestinationRequest(Destination destination, MoneyRequest amount, LabelsArray? labels = null)
{
	public DestinationRequest(Script scriptPubKey, Money amount, bool subtractFee = false, LabelsArray? labels = null)
		: this(scriptPubKey, MoneyRequest.Create(amount, subtractFee), labels)
	{
	}

	public DestinationRequest(Script scriptPubKey, MoneyRequest amount, LabelsArray? labels = null)
		: this(new Destination(scriptPubKey), amount, labels)
	{
	}

	public DestinationRequest(IDestination destination, Money amount, bool subtractFee = false, LabelsArray? labels = null)
		: this(new Destination(destination.ScriptPubKey), MoneyRequest.Create(amount, subtractFee), labels)
	{
	}

	public DestinationRequest(IDestination destination, MoneyRequest amount, LabelsArray? labels = null)
		: this(new Destination(destination.ScriptPubKey), amount, labels)
	{
	}

	public Destination Destination { get; } = destination;
	public MoneyRequest Amount { get; } = amount;
	public LabelsArray Labels { get; } = labels ?? LabelsArray.Empty;
}
