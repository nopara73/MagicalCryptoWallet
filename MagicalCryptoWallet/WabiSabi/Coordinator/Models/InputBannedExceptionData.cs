namespace MagicalCryptoWallet.WabiSabi.Coordinator.Models;

public record InputBannedExceptionData(DateTimeOffset BannedUntil) : ExceptionData;
