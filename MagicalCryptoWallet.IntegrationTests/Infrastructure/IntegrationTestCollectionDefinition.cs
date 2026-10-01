using Xunit;

namespace MagicalCryptoWallet.IntegrationTests.Infrastructure;

/// <summary>
/// The tests in this collection share a Bitcoin Core instance and must run sequentially.
/// </summary>
[CollectionDefinition("Integration tests", DisableParallelization = true)]
public class IntegrationTestCollectionDefinition : ICollectionFixture<IntegrationTestFixture>
{
}