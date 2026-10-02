using MagicalCryptoWallet.Userfacing.Bip21;
using NBitcoinExtensions = MagicalCryptoWallet.Extensions.NBitcoinExtensions;

namespace MagicalCryptoWallet.Userfacing;

using AddressParsingResult = Result<Address, string>;

public abstract record Address
{
	public record Bip21Uri(Address Address, decimal? Amount, string? Label) : Address;
	public record Bitcoin(BitcoinAddress Address) : Address;

	public string ToWif(Network network) =>
		this switch
		{
			Bitcoin bitcoin => bitcoin.Address.ToString(),
			Bip21Uri bip21 => UriToString(bip21),
			_ => throw new ArgumentException("Unknown address type.")
		};

	public string ToCanonicalAddress(Network network) =>
		this switch
		{
			Bip21Uri bip21 => bip21.Address.ToWif(network),
			Bitcoin bitcoin => bitcoin.Address.ToString(),
			_ => throw new ArgumentException("Unknown address type.")
		};

	private static string UriToString(Bip21Uri bip21)
	{
		var parametersArray = new[]
		{
			bip21.Amount is not null ? $"amount={bip21.Amount.Value.ToString(System.Globalization.CultureInfo.InvariantCulture)}" : "",
			bip21.Label is not null ? $"label={Uri.EscapeDataString(bip21.Label)}" : "",
		}.Where(x => x != "");
		var parameterString = string.Join("&", parametersArray);

		var address = ((Bitcoin)bip21.Address).Address;
		return $"bitcoin:{address}" + (parameterString.Length == 0 ? "" : $"?{parameterString}");
	}
}

public static class AddressParser
{
	public static AddressParsingResult Parse(string text, Network expectedNetwork)
	{
		text = text.Trim();

		if (text == "")
		{
			return AddressParsingResult.Fail("Input length is invalid.");
		}

		// Too long URIs/Bitcoin address are unsupported.
		if (text.Length > 1000)
		{
			return AddressParsingResult.Fail("Input is too long.");
		}

		// Parse a Bitcoin address (not BIP21 URI string)
		if (!text.StartsWith($"{Bip21UriParser.UriScheme}:", StringComparison.OrdinalIgnoreCase))
		{
			return ParseBitcoinAddress(text, expectedNetwork);
		}

		// Parse BIP21 URI string.
		if (!Bip21UriParser.TryParse(input: text, expectedNetwork, out var result, out var error))
		{
			return AddressParsingResult.Fail(error.Message);
		}

		return AddressParsingResult.Ok(
			new Address.Bip21Uri(
				result.Address,
				result.Amount?.ToDecimal(MoneyUnit.BTC),
				result.Label));
	}

	public static AddressParsingResult ParseBitcoinAddress(string text, Network expectedNetwork)
	{
		if (NBitcoinExtensions.TryParseBitcoinAddressForNetwork(text, expectedNetwork, out BitcoinAddress? address))
		{
			return AddressParsingResult.Ok(new Address.Bitcoin(address));
		}

		return AddressParsingResult.Fail(Bip21UriParser.ErrorInvalidAddress.Message);
	}
}
