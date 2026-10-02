using System.Collections.Immutable;
using System.Linq;
using System.Reflection;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Crypto;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Tests.UnitTests.Services;
using MagicalCryptoWallet.WabiSabi.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Manager;
using MagicalCryptoWallet.WabiSabi.Client.RoundStateAwaiters;
using MagicalCryptoWallet.WabiSabi.Client.StatusChangedEvents;
using MagicalCryptoWallet.WabiSabi.Coordinator;
using MagicalCryptoWallet.WabiSabi.Coordinator.Models;
using MagicalCryptoWallet.WabiSabi.Coordinator.PostRequests;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.WabiSabi.Models;
using MagicalCryptoWallet.WabiSabi.Models.MultipartyTransaction;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.WabiSabi.Client;

public class WalletInputValidationTests
{
	[Theory]
	[InlineData("amount")]
	[InlineData("script")]
	[InlineData("missing")]
	public async Task AlteredWalletInputIsRejectedBeforeOutputRegistrationAsync(string alteration)
	{
		var keys = ServiceFactory.CreateKeyManager("");
		var (keyChain, registeredCoin, otherCoin) = WabiSabiFactory.CreateCoinKeyPairs(keys);
		var round = WabiSabiFactory.CreateRound(new WabiSabiConfig());
		var parameters = round.Parameters;
		var commitment = new CoinJoinInputCommitmentData(parameters.CoordinationIdentifier, round.Id);
		var construction = new ConstructionState(parameters);
		if (alteration != "missing")
		{
			var advertisedOutput = new TxOut(
				alteration == "amount" ? registeredCoin.Amount - Money.Satoshis(1) : registeredCoin.Amount,
				alteration == "script" ? otherCoin.ScriptPubKey : registeredCoin.ScriptPubKey);
			var advertisedCoin = new Coin(registeredCoin.Outpoint, advertisedOutput);
			var proof = keyChain.GetOwnershipProof(alteration == "script" ? otherCoin : registeredCoin, commitment);
			construction = construction.AddInput(advertisedCoin, proof, commitment);
		}
		round.CoinjoinState = construction;
		round.SetPhase(Phase.OutputRegistration);
		var state = RoundState.FromRound(round);
		var handler = DispatchProxy.Create<IWabiSabiApiRequestHandler, WalletInputStatusHandler>();
		((WalletInputStatusHandler)handler).State = state;
		using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
		using var updater = RoundStateUpdaterForTesting.CreateManual(handler, timeout.Token);
		var client = WabiSabiFactory.CreateTestCoinJoinClient(_ => handler, keys, new RoundStateProvider(updater));
		var arena = new ArenaClient(state.CreateAmountCredentialClient(InsecureRandom.Instance),
			state.CreateVsizeCredentialClient(InsecureRandom.Instance), parameters.CoordinationIdentifier, handler);
		var constructor = typeof(AliceClient).GetConstructors(BindingFlags.Instance | BindingFlags.NonPublic).Single();
		var credentialType = constructor.GetParameters()[4].ParameterType.GetGenericArguments()[0];
		var emptyCredentials = Array.CreateInstance(credentialType, 0);
		var alice = (AliceClient)constructor.Invoke([Guid.NewGuid(), state, arena, registeredCoin, emptyCredentials, emptyCredentials]);
		var method = typeof(CoinJoinClient).GetMethod("ProceedWithOutputRegistrationPhaseAsync", BindingFlags.Instance | BindingFlags.NonPublic)!;
		var registration = (Task<TxOut[]>)method.Invoke(client,
			[round.Id, ImmutableArray.Create(alice), CoinJoinClient.UnrestrictedRound.Instance, timeout.Token])!;
		updater.Post(new RoundUpdateMessage.UpdateMessage(DateTime.UtcNow));

		var error = await Assert.ThrowsAsync<CoinJoinClientException>(() => registration);
		Assert.Equal(CoinjoinError.CoordinatorLiedAboutInputs, error.CoinjoinError);
	}
}

public class WalletInputStatusHandler : DispatchProxy
{
	public RoundState State { get; set; } = null!;

	protected override object? Invoke(MethodInfo? targetMethod, object?[]? args) => targetMethod?.Name == nameof(IWabiSabiApiRequestHandler.GetStatusAsync)
		? Task.FromResult(new RoundStateResponse([State]))
		: throw new InvalidOperationException($"An altered input reached a coordinator operation: {targetMethod?.Name}.");
}
