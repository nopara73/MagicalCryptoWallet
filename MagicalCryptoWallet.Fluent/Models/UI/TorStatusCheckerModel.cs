using System.Collections.Generic;
using System.Linq;
using System.Reactive.Linq;
using MagicalCryptoWallet.Fluent.Extensions;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Tor.StatusChecker;

namespace MagicalCryptoWallet.Fluent.Models.UI;

public partial class TorStatusCheckerModel
{
	public TorStatusCheckerModel(IServices services)
	{
		Issues = services.EventBus
			.AsObservable<TorNetworkStatusChanged>()
			.Select(e => e.ReportedIssues.ToList());
	}

	public IObservable<IList<Issue>> Issues { get; }
}
