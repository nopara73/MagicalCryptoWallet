using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Backend.Models;

namespace MagicalCryptoWallet.Stores;

public interface IFilterStore
{
	Task<FilterModel[]> FetchBatchAsync(uint fromHeight, int batchSize, CancellationToken cancellationToken);
}
