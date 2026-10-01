using System.Threading.Tasks;

namespace MagicalCryptoWallet.Fluent.Models.Wallets;

public interface IHardwareWalletModel : IWalletModel
{
	Task<bool> AuthorizeTransactionAsync(TransactionAuthorizationInfo transactionAuthorizationInfo);
}
