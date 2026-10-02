namespace MagicalCryptoWallet.WabiSabi.Models;

public record ReadyToSignRequestRequest(
	uint256 RoundId,
	Guid AliceId);
