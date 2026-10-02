using System.Net.Http;

namespace MagicalCryptoWallet.Mcw.Network;

/// <summary>
/// Application-owned named-client contract for the retained managed HTTP transport.
/// Factories preserve identity isolation and handler lifetimes; callers own each client.
/// </summary>
public interface IMcwHttpClientFactory
{
	HttpClient CreateClient(string name);
}
