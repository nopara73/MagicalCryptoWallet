using System.Collections.Generic;
using System.Net.Http;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Analysis.Clustering;
using MagicalCryptoWallet.Blockchain.Blocks;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Client.Configuration;
using MagicalCryptoWallet.Models;
using MagicalCryptoWallet.Services;
using MagicalCryptoWallet.Wallets;

namespace MagicalCryptoWallet.Fluent;

public interface IServices
{
	string DataDir { get; }
	string PersistentConfigFilePath { get; }
	PersistentConfig PersistentConfig { get; }
	WalletSession WalletSession { get; }
	UiConfig UiConfig { get; }
	Config Config { get; }
	EventBus EventBus { get; }

	uint GetTipHeight();
	uint GetServerTipHeight();
	int GetHashesLeft();
	SmartHeader? GetTip();
	uint GetBlockHeadersTipHeight();
	int GetPeerCount();

	uint? GetMinimumBlockHeight();

	IEnumerable<LabelsArray> GetTransactionLabels();

	Network GetNetwork();






	string GetTorLogFilePath();
	TorMode GetUseTor();

	decimal GetUsdExchangeRate();

	bool GetHideOnClose();
	double? GetWindowWidth();
	double? GetWindowHeight();
	void SetWindowWidth(double? width);
	void SetWindowHeight(double? height);
	bool GetPrivacyMode();
	bool GetAutocopy();
	bool GetAutoPaste();
	bool GetSendAmountConversionReversed();
	void SetSendAmountConversionReversed(bool value);

	T? GetHostedService<T>() where T : class, Microsoft.Extensions.Hosting.IHostedService;

	Task SendTransactionAsync(SmartTransaction transaction);

	HttpClient CreateHttpClient(string name);

	bool IsForcefulTerminationRequested();
}
