using System.Reactive.Linq;
using MagicalCryptoWallet.Client.Application;

namespace MagicalCryptoWallet.Fluent.Models.UI;

public partial class QrCodeGenerator
{
	public IObservable<bool[,]> Generate(string data)
	{
		return Observable.FromAsync(cancellationToken =>
			(ManagedApplicationHost.Current ?? throw new InvalidOperationException("The mcw application host is not connected."))
				.GenerateQrAsync(data, cancellationToken: cancellationToken));
	}
}
