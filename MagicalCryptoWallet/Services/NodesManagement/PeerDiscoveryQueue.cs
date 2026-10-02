using System.Collections.Generic;
using System.Linq;
using System.Net;

namespace MagicalCryptoWallet.Services.NodesManagement;

/// <summary>Bounds discovery work and prevents repeated or concurrent probes of an endpoint.</summary>
public sealed class PeerDiscoveryQueue(int capacity = 2_048)
{
	private readonly Queue<EndPoint> _pending = new();
	private readonly HashSet<EndPoint> _queued = [];
	private readonly HashSet<EndPoint> _active = [];
	private readonly Dictionary<EndPoint, DateTimeOffset> _retryAfter = [];

	public int Count => _pending.Count;

	public void Enqueue(IEnumerable<EndPoint> endpoints, DateTimeOffset now)
	{
		foreach (var endpoint in endpoints.Select(PeerConnectionRegistry.Normalize))
		{
			if (_pending.Count >= capacity) { break; }
			if (_active.Contains(endpoint) || (_retryAfter.TryGetValue(endpoint, out var next) && now < next) || !_queued.Add(endpoint)) { continue; }
			_pending.Enqueue(endpoint);
		}
	}

	public bool TryDequeue(out EndPoint endpoint)
	{
		if (!_pending.TryDequeue(out endpoint!)) { return false; }
		_queued.Remove(endpoint);
		_active.Add(endpoint);
		return true;
	}

	public void Complete(EndPoint endpoint, DateTimeOffset now, bool succeeded)
	{
		endpoint = PeerConnectionRegistry.Normalize(endpoint);
		_active.Remove(endpoint);
		_retryAfter[endpoint] = now + (succeeded ? TimeSpan.FromHours(6) : TimeSpan.FromMinutes(5));
		if (_retryAfter.Count > capacity * 2)
		{
			foreach (var expired in _retryAfter.Where(x => x.Value <= now).Select(x => x.Key).ToArray()) { _retryAfter.Remove(expired); }
			foreach (var oldest in _retryAfter.OrderBy(x => x.Value).Take(_retryAfter.Count - capacity * 2).Select(x => x.Key).ToArray()) { _retryAfter.Remove(oldest); }
		}
	}
}
