using NBitcoin;
using System.Threading;
using System.Threading.Tasks;
using System.Linq;
using MagicalCryptoWallet.Crypto;
using MagicalCryptoWallet.Crypto.Randomness;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.WabiSabi.Models;
using Xunit;
using MagicalCryptoWallet.WabiSabi.Client.CoinJoin.Client;
using MagicalCryptoWallet.WabiSabi.Coordinator;
using MagicalCryptoWallet.WabiSabi.Coordinator.Models;
using MagicalCryptoWallet.WabiSabi.Coordinator.Rounds;
using MagicalCryptoWallet.Coordinator.WabiSabi;

namespace MagicalCryptoWallet.Tests.UnitTests.WabiSabi.Backend.PostRequests;

public class RegisterInputSuccessTests
{
	private static void AssertSingleAliceSuccessfullyRegistered(Round round, DateTimeOffset minAliceDeadline, ArenaResponse<Guid> resp)
	{
		var alice = Assert.Single(round.Alices);
		Assert.NotNull(resp);
		Assert.NotNull(resp.IssuedAmountCredentials);
		Assert.NotNull(resp.IssuedVsizeCredentials);
		Assert.True(minAliceDeadline <= alice.Deadline);
	}

	[Fact]
	public async Task SuccessAsync()
	{
		WabiSabiConfig cfg = new();
		var round = WabiSabiFactory.CreateRound(cfg);

		using Key key = new();
		var coin = WabiSabiFactory.CreateCoin(key);
		var rpc = WabiSabiFactory.CreatePreconfiguredRpcClient(coin);
		using Arena arena = await ArenaBuilder.From(cfg).With(rpc).CreateAndStartAsync(round);

		var minAliceDeadline = DateTimeOffset.UtcNow + cfg.ConnectionConfirmationTimeout * 0.9;
		var arenaClient = WabiSabiFactory.CreateArenaClient(arena);
		var ownershipProof = WabiSabiFactory.CreateOwnershipProof(key, round.Id);

		var resp = await arenaClient.RegisterInputAsync(round.Id, coin.Outpoint, ownershipProof, CancellationToken.None);
		AssertSingleAliceSuccessfullyRegistered(round, minAliceDeadline, resp);

		await arena.StopAsync(CancellationToken.None);
	}

	[Fact]
	public async Task SuccessCustomCoordinatorIdentifierAsync()
	{
		WabiSabiConfig cfg = new();
		cfg.CoordinatorIdentifier = "test";
		var round = WabiSabiFactory.CreateRound(cfg);

		using Key key = new();
		var coin = WabiSabiFactory.CreateCoin(key);
		var rpc = WabiSabiFactory.CreatePreconfiguredRpcClient(coin);
		using Arena arena = await ArenaBuilder.From(cfg).With(rpc).CreateAndStartAsync(round);

		var minAliceDeadline = DateTimeOffset.UtcNow + cfg.ConnectionConfirmationTimeout * 0.9;

		var roundState = RoundState.FromRound(arena.Rounds.First());
		var arenaClient = new ArenaClient(
			roundState.CreateAmountCredentialClient(InsecureRandom.Instance),
			roundState.CreateVsizeCredentialClient(InsecureRandom.Instance),
			"test",
			arena);
		var ownershipProof = OwnershipProof.GenerateCoinJoinInputProof(key, new OwnershipIdentifier(key, key.PubKey.GetScriptPubKey(ScriptPubKeyType.Segwit)), new CoinJoinInputCommitmentData("test", round.Id), ScriptPubKeyType.Segwit);

		var resp = await arenaClient.RegisterInputAsync(round.Id, coin.Outpoint, ownershipProof, CancellationToken.None);
		AssertSingleAliceSuccessfullyRegistered(round, minAliceDeadline, resp);

		await arena.StopAsync(CancellationToken.None);
	}

	[Fact]
	public async Task SuccessWithAliceUpdateIntraRoundAsync()
	{
		WabiSabiConfig cfg = new();
		var round = WabiSabiFactory.CreateRound(cfg);

		using Key key = new();
		var ownershipProof = WabiSabiFactory.CreateOwnershipProof(key, round.Id);
		var coin = WabiSabiFactory.CreateCoin(key);

		// Make sure an Alice have already been registered with the same input.
		var preAlice = WabiSabiFactory.CreateAlice(coin, WabiSabiFactory.CreateOwnershipProof(key));
		round.Alices.Add(preAlice);

		var rpc = WabiSabiFactory.CreatePreconfiguredRpcClient(coin);
		using Arena arena = await ArenaBuilder.From(cfg).With(rpc).CreateAndStartAsync(round);

		var arenaClient = WabiSabiFactory.CreateArenaClient(arena);
		var ex = await Assert.ThrowsAsync<WabiSabiProtocolException>(async () => await arenaClient.RegisterInputAsync(round.Id, coin.Outpoint, ownershipProof, CancellationToken.None).ConfigureAwait(false));
		Assert.Equal(WabiSabiProtocolErrorCode.AliceAlreadyRegistered, ex.ErrorCode);

		await arena.StopAsync(CancellationToken.None);
	}

	[Fact]
	public async Task TaprootSuccessAsync()
	{
		WabiSabiConfig cfg = new() { AllowP2trInputs = true };
		var round = WabiSabiFactory.CreateRound(cfg);

		using Key key = new();
		var coin = WabiSabiFactory.CreateCoin(key, scriptPubKeyType: ScriptPubKeyType.TaprootBIP86);
		var rpc = WabiSabiFactory.CreatePreconfiguredRpcClient(coin);
		using Arena arena = await ArenaBuilder.From(cfg).With(rpc).CreateAndStartAsync(round);

		var minAliceDeadline = DateTimeOffset.UtcNow + cfg.ConnectionConfirmationTimeout * 0.9;
		var arenaClient = WabiSabiFactory.CreateArenaClient(arena);
		var ownershipProof = WabiSabiFactory.CreateOwnershipProof(key, round.Id, ScriptPubKeyType.TaprootBIP86);

		var resp = await arenaClient.RegisterInputAsync(round.Id, coin.Outpoint, ownershipProof, CancellationToken.None);
		AssertSingleAliceSuccessfullyRegistered(round, minAliceDeadline, resp);

		await arena.StopAsync(CancellationToken.None);
	}
}
