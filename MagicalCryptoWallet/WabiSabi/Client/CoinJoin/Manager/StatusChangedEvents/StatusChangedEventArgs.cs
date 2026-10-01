namespace MagicalCryptoWallet.WabiSabi.Client.StatusChangedEvents;

public enum StopReason
{
	WalletUnloaded
}

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

public class StatusChangedEventArgs : EventArgs
{
	public StatusChangedEventArgs(Wallet wallet)
	{
		Wallet = wallet;
	}

	public Wallet Wallet { get; }
}
