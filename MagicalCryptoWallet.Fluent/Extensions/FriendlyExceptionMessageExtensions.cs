using System.Collections.Generic;
using System.Net.Http;
using MagicalCryptoWallet.BitcoinRpc;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Fluent.Extensions;

public static class FriendlyExceptionMessageExtensions
{
	public static string ToUserFriendlyString(this Exception ex)
	{
		var exceptionMessage = Guard.Correct(ex.Message);

		if (exceptionMessage.Length == 0)
		{
			return "An unexpected error occurred. Please try again or contact support.";
		}

		if (TryFindRpcErrorMessage(exceptionMessage, out var friendlyMessage))
		{
			return friendlyMessage;
		}

		return ex switch
		{
			HttpRequestException => "Something went wrong. Please try again.",
			UnauthorizedAccessException => "Magical Crypto Wallet was unable to perform this action due to a lack of permission.",
			_ => ex.Message
		};
	}

	private static bool TryFindRpcErrorMessage(string exceptionMessage, out string friendlyMessage)
	{
		friendlyMessage = "";

		foreach (KeyValuePair<string, string> pair in RpcErrorTools.ErrorTranslations)
		{
			if (exceptionMessage.Contains(pair.Key, StringComparison.InvariantCultureIgnoreCase))
			{
				{
					friendlyMessage = pair.Value;
					return true;
				}
			}
		}

		return false;
	}
}
