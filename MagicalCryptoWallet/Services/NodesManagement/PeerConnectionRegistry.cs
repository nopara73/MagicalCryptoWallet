using System.Collections.Concurrent;
using System.Collections.Generic;
using System.Net;

namespace MagicalCryptoWallet.Services.NodesManagement;

/// <summary>A peer cannot simultaneously belong to the public and protected connection pools.</summary>
public sealed class PeerConnectionRegistry
{
	private readonly ConcurrentDictionary<EndPoint, string> _owners = new();

	public bool TryReserve(EndPoint endpoint, string owner) => _owners.TryAdd(HostKey(endpoint), owner);
	public bool IsReserved(EndPoint endpoint) => _owners.ContainsKey(HostKey(endpoint));
	public void Release(EndPoint endpoint, string owner) => _owners.TryRemove(new KeyValuePair<EndPoint, string>(HostKey(endpoint), owner));

	private static EndPoint HostKey(EndPoint endpoint) => Normalize(endpoint) switch
	{
		IPEndPoint ip => new IPEndPoint(ip.Address, 0),
		DnsEndPoint dns => new DnsEndPoint(dns.Host, 0),
		var normalized => normalized
	};

	public static EndPoint Normalize(EndPoint endpoint) => endpoint switch
	{
		IPEndPoint ip => new IPEndPoint(ip.Address.IsIPv4MappedToIPv6 ? ip.Address.MapToIPv4() : ip.Address, ip.Port),
		DnsEndPoint dns => new DnsEndPoint(dns.Host.ToLowerInvariant(), dns.Port),
		_ => endpoint
	};
}
