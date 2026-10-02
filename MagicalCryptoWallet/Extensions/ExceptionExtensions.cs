using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Models;

namespace MagicalCryptoWallet.Extensions;

public static class ExceptionExtensions
{
	public static string ToTypeMessageString(this Exception ex)
	{
		var trimmed = Guard.Correct(ex.Message);

		if (trimmed.Length == 0)
		{
			return ex.GetType().Name;
		}
		else
		{
			return $"{ex.GetType().Name}: {ex.Message}";
		}
	}

	public static SerializableException ToSerializableException(this Exception ex)
	{
		return new SerializableException(ex);
	}
}
