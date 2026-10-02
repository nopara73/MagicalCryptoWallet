using System.Collections.Generic;
using System.IO;
using System.Runtime.CompilerServices;
using System.Threading.Tasks;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Tests.Helpers;

public static class Common
{
	private static readonly string RunId = Guid.NewGuid().ToString("N");
	public static string DataDir => EnvironmentHelpers.GetDataDir(Path.Combine("MagicalCryptoWallet", "Tests", RunId));

	public static string GetWorkDir([CallerFilePath] string callerFilePath = "", [CallerMemberName] string callerMemberName = "")
	{
		return Path.Combine(DataDir, EnvironmentHelpers.ExtractFileName(callerFilePath), callerMemberName);
	}

	/// <summary>
	/// Gets an empty directory for test to work with.
	/// </summary>
	/// <remarks>If the directory exists, its content is removed.</remarks>
	public static async Task<string> GetEmptyWorkDirAsync([CallerFilePath] string callerFilePath = "", [CallerMemberName] string callerMemberName = "")
	{
		string workDirectory = GetWorkDir(callerFilePath, callerMemberName);

		if (Directory.Exists(workDirectory))
		{
			await IoHelpers.TryDeleteDirectoryAsync(workDirectory).ConfigureAwait(false);
		}

		Directory.CreateDirectory(workDirectory);

		return workDirectory;
	}

	public static IEnumerable<TResult> Repeat<TResult>(Func<TResult> action, int count)
	{
		for (int i = 0; i < count; i++)
		{
			yield return action();
		}
	}
}
