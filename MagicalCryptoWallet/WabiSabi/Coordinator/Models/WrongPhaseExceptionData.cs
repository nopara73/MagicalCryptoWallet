using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;

namespace MagicalCryptoWallet.WabiSabi.Coordinator.Models;

public record WrongPhaseExceptionData(Phase CurrentPhase) : ExceptionData;
