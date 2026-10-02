using System.Collections.Generic;
using System.IO;
using System.Runtime.CompilerServices;
using System.Threading.Tasks;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Tests.Helpers;

public static class Common
{
	private static readonly string RunId = Guid.NewGuid().ToString("N");
	private static string? SyntheticDataRoot { get; set; }
	public static string DataDir => SyntheticDataRoot is { } root
		? Path.Combine(root, "MagicalCryptoWallet", "Tests", RunId)
		: EnvironmentHelpers.GetDataDir(Path.Combine("MagicalCryptoWallet", "Tests", RunId));

	// Verification children can confine retained fixtures without changing the
	// user's home directory or the application's storage-path implementation.
	internal static void UseSyntheticDataRoot(string directory)
	{
		SyntheticDataRoot = Path.GetFullPath(directory);
		Directory.CreateDirectory(DataDir);
	}

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
