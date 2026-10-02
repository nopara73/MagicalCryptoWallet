using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.WabiSabi.Crypto;
using MagicalCryptoWallet.WabiSabi.Models;

namespace MagicalCryptoWallet.Tests.Helpers;

// State/awaiter unit tests inject the retained coordinator oracle. The separate
// RoundHashProbe exercises the default production client through the real host.
public static class RoundHashReference
{
	public static Task<bool> MatchesAsync(RoundState round, CancellationToken cancellationToken)
	{
		cancellationToken.ThrowIfCancellationRequested();
		var p = round.CoinjoinState.Parameters;
		var hash = RoundHasher.CalculateHash(round.InputRegistrationStart, round.InputRegistrationTimeout,
			p.ConnectionConfirmationTimeout, p.OutputRegistrationTimeout, p.TransactionSigningTimeout,
			p.AllowedInputAmounts, p.AllowedInputTypes, p.AllowedOutputAmounts, p.AllowedOutputTypes, p.Network,
			p.MiningFeeRate.FeePerK, p.MaxTransactionSize, p.MinRelayTxFee.FeePerK, p.MaxAmountCredentialValue,
			p.MaxVsizeCredentialValue, p.MaxVsizeAllocationPerAlice, p.MaxSuggestedAmount, p.CoordinationIdentifier,
			round.AmountCredentialIssuerParameters, round.VsizeCredentialIssuerParameters);
		return Task.FromResult(round.Id == hash);
	}
}
