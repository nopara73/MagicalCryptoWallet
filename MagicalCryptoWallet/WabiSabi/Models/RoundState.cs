using WabiSabi.Crypto;
using WabiSabi.Crypto.Randomness;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.Mcw.CoinJoin;
using MagicalCryptoWallet.WabiSabi.Models.MultipartyTransaction;
using CredentialIssuerParameters = WabiSabi.Crypto.CredentialIssuerParameters;

namespace MagicalCryptoWallet.WabiSabi.Models;

public record RoundState(uint256 Id,
	uint256 BlameOf,
	CredentialIssuerParameters AmountCredentialIssuerParameters,
	CredentialIssuerParameters VsizeCredentialIssuerParameters,
	Phase Phase,
	EndRoundState EndRoundState,
	DateTimeOffset InputRegistrationStart,
	TimeSpan InputRegistrationTimeout,
	MultipartyTransactionState CoinjoinState)
{
	public async Task<bool> IsRoundIdMatchingAsync(CancellationToken cancellationToken = default) =>
		Id == await McwRoundHash.CalculateAsync(this, cancellationToken).ConfigureAwait(false);
	public DateTimeOffset InputRegistrationEnd => InputRegistrationStart + InputRegistrationTimeout;
	public bool IsBlame => BlameOf != uint256.Zero;

	public static RoundState FromRound(Round round, int stateId = 0) =>
		new(
			round.Id,
			round is BlameRound blameRound ? blameRound.BlameOf.Id : uint256.Zero,
			round.AmountCredentialIssuerParameters,
			round.VsizeCredentialIssuerParameters,
			round.Phase,
			round.EndRoundState,
			round.InputRegistrationTimeFrame.StartTime,
			round.InputRegistrationTimeFrame.Duration,
			round.CoinjoinState.GetStateFrom(stateId)
			);

	public RoundState GetSubState(int skipFromBaseState) =>
		new(
			Id,
			BlameOf,
			AmountCredentialIssuerParameters,
			VsizeCredentialIssuerParameters,
			Phase,
			EndRoundState,
			InputRegistrationStart,
			InputRegistrationTimeout,
			CoinjoinState.GetStateFrom(skipFromBaseState)
			);

	public TState Assert<TState>() where TState : MultipartyTransactionState =>
		CoinjoinState switch
		{
			TState s => s,
			_ => throw new InvalidOperationException($"{typeof(TState).Name} state was expected but {CoinjoinState.GetType().Name} state was received.")
		};

	public WabiSabiClient CreateAmountCredentialClient(WalletRandom random) =>
		new(AmountCredentialIssuerParameters, random, CoinjoinState.Parameters.MaxAmountCredentialValue);

	public WabiSabiClient CreateVsizeCredentialClient(WalletRandom random) =>
		new(VsizeCredentialIssuerParameters, random, CoinjoinState.Parameters.MaxVsizeCredentialValue);
}
