using System.IO;
using System.Linq;
using System.Security;
using System.Threading.Tasks;
using NBitcoin;
using Newtonsoft.Json.Linq;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Client;
using MagicalCryptoWallet.Client.Rpc;
using MagicalCryptoWallet.Rpc;
using MagicalCryptoWallet.Tests.Helpers;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests;

public class WalletOperationAuthorizationTests
{
	[Fact]
	public async Task UnacceptedUiAuthorizationCannotStartCoinJoinAndWrongPasswordsRemainRejectedAsync()
	{
		await using var app = new SingleWalletTests.SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var wallet = app.Session.Configure(app.NewKeys("secret"));
		var model = new MagicalCryptoWallet.Fluent.Models.Wallets.WalletAuthorizationModel(wallet);
		using (var dismissed = await model.TryAuthorizeAsync("secret"))
		{
			Assert.NotNull(dismissed);
			Assert.True(app.Session.Snapshot.CoinJoinRequiresAuthorization);
			Assert.Null(app.Session.CoinJoinKeyChain);
		}
		Assert.True(app.Session.Snapshot.CoinJoinRequiresAuthorization);
		using var accepted = await model.TryAuthorizeAsync("secret");
		Assert.NotNull(accepted);
		app.Session.CompleteOperationAuthorization(accepted);
		Assert.False(app.Session.Snapshot.CoinJoinRequiresAuthorization);
		Assert.NotNull(app.Session.CoinJoinKeyChain);
		Assert.Null(await model.TryAuthorizeAsync("wrong"));
	}
	[Theory]
	[InlineData("secret")]
	[InlineData("")]
	public async Task PublicPreviewSignsExactlyReviewedTransactionAndChecksEveryPasswordAsync(string password)
	{
		await using var app = new SingleWalletTests.SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var wallet = app.Session.Configure(app.NewKeys(password));
		var coins = ServiceFactory.CreateCoins(wallet.KeyManager, [("synthetic", 0, 0.02m, true, 1)]);
		foreach (var coin in coins) { wallet.TransactionProcessor.Process(coin.Transaction); wallet.TransactionStore.AddOrUpdate(coin.Transaction); }
		await app.InitializeAsync();
		await SingleWalletTests.WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
		using var recipient = new Key();
		var destination = recipient.PubKey.GetAddress(ScriptPubKeyType.Segwit, Network.RegTest);
		var preview = wallet.BuildTransaction(new Destination(destination.ScriptPubKey), Money.Coins(0.005m), "reviewed", new FeeRate(2m), wallet.Coins, subtractFee: false);
		Assert.False(preview.Signed);
		var reviewed = preview.Psbt.GetGlobalTransaction().ToHex();
		app.Session.AuthorizeCoinJoin(password);
		Assert.Throws<SecurityException>(() => WalletAuthorization.Create(wallet.KeyManager, "wrong"));
		using var authorization = WalletAuthorization.Create(wallet.KeyManager, password);
		var signed = authorization.Sign(preview);
		Assert.True(signed.Signed);
		Assert.Equal(reviewed, signed.Psbt.GetGlobalTransaction().ToHex());
		Assert.Equal(preview.Fee, signed.Fee);
		Assert.True(Network.RegTest.CreateTransactionBuilder().AddCoins(coins.Select(x => x.Coin)).Verify(signed.Transaction.Transaction));
		authorization.Dispose();
		Assert.Throws<ObjectDisposedException>(() => authorization.Sign(preview));
		var rpc = new MagicalCryptoWalletJsonRpcService(app.Global);
		var payments = new[] { new PaymentInfo { Sendto = new Destination(destination.ScriptPubKey), Amount = Money.Coins(0.005m), Label = "test" } };
		Assert.Throws<SecurityException>(() => rpc.BuildTransaction(payments, feeRate: 2m, password: "wrong"));
		app.Connected = false;
		await SingleWalletTests.WaitForAsync(() => app.Session.Snapshot.State == WalletSessionState.Offline);
		Assert.Single(rpc.GetUnspentCoinList());
		Assert.False((bool)rpc.WalletInfo()["synchronized"]!);
		Assert.NotNull(rpc.WalletInfo()["balance"]);
		Assert.Throws<InvalidOperationException>(() => rpc.BuildTransaction(payments, feeRate: 2m, password: password));
	}

	[Fact]
	public async Task LegacyPublicAccountsRemainIncompleteUntilInteractiveAuthorizationAsync()
	{
		await using var app = new SingleWalletTests.SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var draft = app.NewKeys("secret");
		draft.ToFile();
		var json = JObject.Parse(await File.ReadAllTextAsync(draft.FilePath!));
		json.Remove("TaprootExtPubKey");
		await File.WriteAllTextAsync(draft.FilePath!, json.ToString());
		var keys = KeyManager.FromFile(draft.FilePath!);
		File.Delete(draft.FilePath!);
		app.Session.Configure(keys);
		await app.InitializeAsync();
		await SingleWalletTests.WaitForAsync(() => app.Session.Snapshot.HasCachedData);
		Assert.True(app.Session.Snapshot.PublicMetadataRequiresAuthorization);
		Assert.False(app.Session.Snapshot.IsSynchronized);
		using var authorization = WalletAuthorization.Create(keys, "secret");
		app.Session.CompleteOperationAuthorization(authorization);
		Assert.False(app.Session.Snapshot.PublicMetadataRequiresAuthorization);
		Assert.False(app.Session.Snapshot.CoinJoinRequiresAuthorization);
		await SingleWalletTests.WaitForAsync(() => app.Session.Snapshot.IsSynchronized);
		Assert.NotNull(KeyManager.FromFile(keys.FilePath!).TaprootExtPubKey);
	}

	[Fact]
	public async Task SchemeStatusWorksBeforeSetupAndRetiredExportsFailAsync()
	{
		await using var app = new SingleWalletTests.SyntheticApplication(await Common.GetEmptyWorkDirAsync());
		var scheme = new Scheme(app.Global);
		var status = JObject.Parse(scheme.ToJson(await scheme.ExecuteAsync("(wallet-info)")));
		Assert.Equal("Unconfigured", status["state"]!.Value<string>());
		foreach (var expression in new[] { "(open-wallet)", "(__start_wallet)", "(wallet-name (wallet))" })
		{ await Assert.ThrowsAnyAsync<Exception>(() => scheme.ExecuteAsync(expression)); }
	}
}
