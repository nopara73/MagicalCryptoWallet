using MagicalCryptoWallet.WabiSabi.Models;

namespace MagicalCryptoWallet.WabiSabi.Client.CoinJoinProgressEvents;

public class EnteringInputRegistrationPhase : RoundStateChanged
{
	public EnteringInputRegistrationPhase(RoundState roundState, DateTimeOffset timeoutAt) : base(roundState, timeoutAt)
	{
	}
}
