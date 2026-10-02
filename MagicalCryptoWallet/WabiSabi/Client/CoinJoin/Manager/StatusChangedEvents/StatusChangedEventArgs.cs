namespace MagicalCryptoWallet.WabiSabi.Client.StatusChangedEvents;

public enum CompletionStatus
{
	Success,
	Canceled,
	Failed,
	Unknown
}

public enum CoinjoinError
{
	NoCoinsEligibleToMix,
	AutoConjoinDisabled,
	UserInSendWorkflow,
	NotEnoughUnprivateBalance,
	AllCoinsPrivate,
	UserWasntInRound,
	NoConfirmedCoinsEligibleToMix,
	CoinsRejected,
	OnlyImmatureCoinsAvailable,
	MiningFeeRateTooHigh = 10, // Preserve the existing error numbers.
	MinInputCountTooLow,
	CoordinatorLiedAboutInputs,
	NotEnoughConfirmedUnprivateBalance
}

public class StatusChangedEventArgs : EventArgs;
