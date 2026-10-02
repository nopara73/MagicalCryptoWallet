using NBitcoin;
using MagicalCryptoWallet.Userfacing.Bip21;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Userfacing.Bip21;

/// <summary>
/// Tests for <see cref="Bip21UriParser"/>
/// </summary>
public class Bip21UriParserTests
{
	[Fact]
	public void TryParseTests()
	{
		Assert.False(Bip21UriParser.TryParse(input: "", Network.Main, out Bip21UriParser.Result? result, out Bip21UriParser.Error? error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidUri, error);

		Assert.False(Bip21UriParser.TryParse(input: "nfdjksnfjkdsnfjkds", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidUri, error);

		Assert.False(Bip21UriParser.TryParse("18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidUri, error);

		Assert.False(Bip21UriParser.TryParse("18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?amount=20.3", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidUri, error);

		Assert.False(Bip21UriParser.TryParse("mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidUri, error);

		Assert.False(Bip21UriParser.TryParse("mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP?amount=20.3", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidUri, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorMissingAddress, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:?amount=20.3", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorMissingAddress, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?amount=", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorMissingAmountValue, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?amount=XYZ", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidAmountValue, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?amount=100'000", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidAmountValue, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?amount=100,000", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidAmountValue, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP?amount=", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidAddress, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP?amount=XYZ", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidAddress, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP?amount=100'000", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidAddress, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP?amount=100000", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidAddress, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorUnsupportedReqParameter, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP?req-somethingyoudontunderstand=50&req-somethingelseyoudontget=999", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidAddress, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?amount=1&amount=2", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorDuplicateParameter, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?label=a&Label=b", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorDuplicateParameter, error);

		Assert.False(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?message=a&Message=b", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorDuplicateParameter, error);

		// Unknown optional parameters must not be duplicated either.
		Assert.False(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?unknown=first&unknown=second", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorDuplicateParameter, error);

		// Negative amounts are forbidden.
		Assert.False(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?amount=-0.01", Network.Main, out result, out error));
		Assert.Null(result);
		AssertEqualErrors(Bip21UriParser.ErrorInvalidAmountValue, error);

		// Success cases.
		Assert.True(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", result.Address.ToWif(Network.Main));

		Assert.True(Bip21UriParser.TryParse("BITCOIN:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", result.Address.ToWif(Network.Main));

		Assert.True(Bip21UriParser.TryParse("BitCoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", result.Address.ToWif(Network.Main));
		Assert.True(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?label=Luke-Jr", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", result.Address.ToWif(Network.Main));
		Assert.Equal("Luke-Jr", result.Label);

		Assert.True(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?amount=20.3&label=Luke-Jr", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", result.Address.ToWif(Network.Main));
		Assert.Equal("Luke-Jr", result.Label);
		Assert.Equal(Money.Coins(20.3m), result.Amount);

		// Query keys are treated case insensitively; "amount", "Amount", "label", and "Label" are all valid.
		Assert.True(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?Amount=20.3&Label=Luke-Jr", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", result.Address.ToWif(Network.Main));
		Assert.Equal("Luke-Jr", result.Label);
		Assert.Equal(Money.Coins(20.3m), result.Amount);

		// QR codes may use all-uppercase URIs.
		Assert.True(Bip21UriParser.TryParse("BITCOIN:BC1QUFGY354J3KMVUCH987XE4S40836X3H0LG8F5N2", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("bc1qufgy354j3kmvuch987xe4s40836x3h0lg8f5n2", result.Address.ToWif(Network.Main));
		Assert.Null(result.Label);
		Assert.Null(result.Amount);

		Assert.True(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", result.Address.ToWif(Network.Main));
		Assert.Equal("Luke-Jr", result.Label);
		Assert.Equal(Money.Coins(50m), result.Amount);

		Assert.True(Bip21UriParser.TryParse("bitcoin:3EktnHQD7RiAE6uzMj2ZifT9YgRrkSgzQX?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("3EktnHQD7RiAE6uzMj2ZifT9YgRrkSgzQX", result.Address.ToWif(Network.Main));
		Assert.Equal("Luke-Jr", result.Label);
		Assert.Equal(Money.Coins(50m), result.Amount);

		Assert.True(Bip21UriParser.TryParse("bitcoin:bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("bc1qw508d6qejxtdg4y5r3zarvary0c5xw7kv8f3t4", result.Address.ToWif(Network.Main));
		Assert.Equal("Luke-Jr", result.Label);
		Assert.Equal(Money.Coins(50m), result.Amount);

		Assert.True(Bip21UriParser.TryParse("bitcoin:2MzQwSSnBHWHqSAqtTVQ6v47XtaisrJa1Vc?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz", Network.TestNet, out result, out error));
		Assert.Null(error);
		Assert.Equal("2MzQwSSnBHWHqSAqtTVQ6v47XtaisrJa1Vc", result.Address.ToWif(Network.Main));
		Assert.Equal("Luke-Jr", result.Label);
		Assert.Equal(Money.Coins(50m), result.Amount);

		Assert.True(Bip21UriParser.TryParse("bitcoin:tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx?amount=50&label=Luke-Jr&message=Donation%20for%20project%20xyz", Network.TestNet, out result, out error));
		Assert.Null(error);
		Assert.Equal("tb1qw508d6qejxtdg4y5r3zarvary0c5xw7kxpjzsx", result.Address.ToWif(Network.Main));
		Assert.Equal("Luke-Jr", result.Label);
		Assert.Equal(Money.Coins(50m), result.Amount);

		Assert.True(Bip21UriParser.TryParse("bitcoin:18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX?somethingyoudontunderstand=50&somethingelseyoudontget=999", Network.Main, out result, out error));
		Assert.Null(error);
		Assert.Equal("18cBEMRxXHqzWWCxZNtU91F5sbUNKhL5PX", result.Address.ToWif(Network.Main));

		Assert.True(Bip21UriParser.TryParse("bitcoin:mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP?amount=0.02&label=bolt11_example&lightning=lntb20m1pvjluezsp5zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zyg3zygshp58yjmdan79s6qqdhdzgynm4zwqd5d7xmw5fk98klysy043l2ahrqspp5qqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqqqsyqcyq5rqwzqfqypqfpp3x9et2e20v6pu37c5d9vax37wxq72un989qrsgqdj545axuxtnfemtpwkc45hx9d2ft7x04mt8q7y6t0k2dge9e7h8kpy9p34ytyslj3yu569aalz2xdk8xkd7ltxqld94u8h2esmsmacgpghe9k8", Network.TestNet, out result, out error));
		Assert.Null(error);
		Assert.Equal("mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP", result.Address.ToWif(Network.Main));
		Assert.Equal("bolt11_example", result.Label);
		Assert.Equal(Money.Coins(0.02m), result.Amount);

		// Handling of unknown parameters.
		{
			Assert.True(Bip21UriParser.TryParse("bitcoin:mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP?amount=0.02&label=unknown_params&unknown1=1&unknown2=true&unknown3=someValue", Network.TestNet, out result, out error));
			Assert.Null(error);
			Assert.Equal("mk2QpYatsKicvFVuTAQLBryyccRXMUaGHP", result.Address.ToWif(Network.Main));
			Assert.Equal("unknown_params", result.Label);
			Assert.Equal(Money.Coins(0.02m), result.Amount);
			Assert.True(result.UnknownParameters.TryGetValue("unknown1", out var unknown1));
			Assert.Equal("1", unknown1);
			Assert.True(result.UnknownParameters.TryGetValue("unknown2", out var unknown2));
			Assert.Equal("true", unknown2);
			Assert.True(result.UnknownParameters.TryGetValue("unknown3", out var unknown3));
			Assert.Equal("someValue", unknown3);
		}

		// Fallback addresses.
		{
			{
				// Request funds to be paid over lightning to a BOLT 11 invoice with a fallback to on-chain payments (i.e. bc1qp6ejw8ptj9l9pkscmlf8fhhkrrjeawgpyjvtq8).
				// Lightning is not supported by MagicalCryptoWallet, so we will just parse the fallback address and ignore the lightning parameter.
				Assert.True(Bip21UriParser.TryParse("bitcoin:bc1qp6ejw8ptj9l9pkscmlf8fhhkrrjeawgpyjvtq8?lightning=lnbc420bogusinvoice", Network.Main, out result, out error));
				Assert.Null(error);
				Assert.Equal("bc1qp6ejw8ptj9l9pkscmlf8fhhkrrjeawgpyjvtq8", result.Address.ToWif(Network.Main));
				Assert.Null(result.Label);
				Assert.Null(result.Amount);
				Assert.True(result.UnknownParameters.TryGetValue("lightning", out var unknown1));
				Assert.Equal("lnbc420bogusinvoice", unknown1);
			}
		}
	}

	/// <summary>
	/// Helper method that does not compare <see cref="Bip21UriParser.Error.Details"/> property.
	/// </summary>
	private void AssertEqualErrors(Bip21UriParser.Error expected, Bip21UriParser.Error? actual)
	{
		Assert.NotNull(actual);
		Assert.Equal(expected.Code, actual.Code);
		Assert.Equal(expected.Message, actual.Message);
	}
}
