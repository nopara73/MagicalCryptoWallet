using MagicalCryptoWallet.WabiSabi.Coordinator.Models;

namespace MagicalCryptoWallet.WabiSabi.Models;

public record Error(
	string Type,
	string ErrorCode,
	string Description,
	ExceptionData ExceptionData
);
