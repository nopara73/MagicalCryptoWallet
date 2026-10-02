using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net;
using System.Text.Json;
using NBitcoin.Protocol;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Userfacing;

namespace MagicalCryptoWallet.Services.NodesManagement;

/// <summary>Stores bounded, expiring peer hints. Capabilities are rechecked at every handshake.</summary>
public static class PeerAddressCache
{
	private const int MaximumPeers = 128;
	private static readonly TimeSpan MaximumAge = TimeSpan.FromDays(7);
	private sealed record Entry(string Address, ulong Services, DateTimeOffset LastSeen);

	public static PeerInfo[] Load(string path, DateTimeOffset now)
	{
		try
		{
			if (!File.Exists(path) || new FileInfo(path).Length > 128 * 1_024) { return []; }
			var entries = JsonSerializer.Deserialize<Entry[]>(File.ReadAllText(path)) ?? [];
			return entries.OfType<Entry>().Take(MaximumPeers)
				.Where(e => e.LastSeen <= now && now - e.LastSeen < MaximumAge)
				.Select(e => EndPointParser.TryParse(e.Address, out var endpoint)
					? new PeerInfo(PeerConnectionRegistry.Normalize(endpoint), "", 0, (NodeServices)e.Services, 0, TimeSpan.Zero, e.LastSeen, e.LastSeen)
					: null)
				.OfType<PeerInfo>().DistinctBy(p => p.Endpoint).ToArray();
		}
		catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or JsonException or ArgumentException)
		{
			Logger.LogDebug("Ignoring an unreadable peer cache.", ex);
			return [];
		}
	}

	public static void Save(string path, IEnumerable<PeerInfo> peers, DateTimeOffset now)
	{
		var temporary = path + "." + Guid.NewGuid().ToString("N") + ".tmp";
		try
		{
			var entries = peers.Where(p => p.LastSeen <= now && now - p.LastSeen < MaximumAge)
				.OrderByDescending(p => p.Score).ThenByDescending(p => p.LastSeen)
				.DistinctBy(p => PeerConnectionRegistry.Normalize(p.Endpoint)).Take(MaximumPeers)
				.Select(p => new Entry(p.Endpoint.ToString(8333), (ulong)p.Services, p.LastSeen)).ToArray();
			Directory.CreateDirectory(Path.GetDirectoryName(Path.GetFullPath(path))!);
			File.WriteAllText(temporary, JsonSerializer.Serialize(entries));
			File.Move(temporary, path, overwrite: true);
		}
		catch (Exception ex) when (ex is IOException or UnauthorizedAccessException or ArgumentException)
		{
			Logger.LogDebug("Could not save the peer cache.", ex);
		}
		finally
		{
			try { File.Delete(temporary); }
			catch (Exception ex) when (ex is IOException or UnauthorizedAccessException) { Logger.LogDebug(ex); }
		}
	}
}
