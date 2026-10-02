using System.Collections.Generic;
using System.Linq;
using System.Threading;
using NBitcoin;

namespace MagicalCryptoWallet.Blockchain.Mempool;

/// <summary>Shares bounded, expiring download reservations between mempool peers.</summary>
internal sealed class TransactionRequestTracker(int capacity = 50_000)
{
	internal static readonly TimeSpan RequestTimeout = TimeSpan.FromSeconds(30);
	private readonly Lock _gate = new();
	private readonly Dictionary<uint256, (Guid Owner, DateTimeOffset Expires)> _requests = [];
	private DateTimeOffset _lastPrune;

	public bool TryRequest(uint256 hash, Guid owner, DateTimeOffset now)
	{
		lock (_gate)
		{
			if (now - _lastPrune >= TimeSpan.FromSeconds(1))
			{
				foreach (var expired in _requests.Where(x => x.Value.Expires <= now).Select(x => x.Key).ToArray()) { _requests.Remove(expired); }
				_lastPrune = now;
			}
			if (_requests.TryGetValue(hash, out var request) && request.Expires > now) { return false; }
			if (_requests.Count >= capacity && !_requests.ContainsKey(hash)) { return false; }
			_requests[hash] = (owner, now + RequestTimeout);
			return true;
		}
	}

	public void Complete(uint256 hash)
	{
		lock (_gate) { _requests.Remove(hash); }
	}

	public void Release(uint256 hash, Guid owner)
	{
		lock (_gate)
		{
			if (_requests.TryGetValue(hash, out var request) && request.Owner == owner) { _requests.Remove(hash); }
		}
	}

	public void ReleaseOwner(Guid owner)
	{
		lock (_gate)
		{
			foreach (var hash in _requests.Where(x => x.Value.Owner == owner).Select(x => x.Key).ToArray()) { _requests.Remove(hash); }
		}
	}
}
