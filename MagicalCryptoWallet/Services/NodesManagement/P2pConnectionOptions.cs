namespace MagicalCryptoWallet.Services.NodesManagement;

public record P2pConnectionOptions
{
	public string Name { get; init; } = "wallet";
	public int TargetConnections { get; init; } = 6;
	public int MinimumCompactFilterNodes { get; init; } = 5;
	public bool RelayTransactions { get; init; } = true;
	public bool AllowBlockDownloads { get; init; } = true;
	public bool AllowTransactionBroadcasts { get; init; } = true;
	public string? PeerCacheFile { get; init; }
}
