using System.IO;
using System.Collections.Concurrent;
using System.Text;

namespace MagicalCryptoWallet.Bases;

public abstract class ConfigBase : NotifyPropertyChangedBase
{
	protected ConfigBase(string filePath)
	{
		FilePath = filePath;
	}

	// Reloads and background saves share the lock even when they use different config instances.
	private static readonly ConcurrentDictionary<string, Lock> FileLocks = new(
		OperatingSystem.IsWindows() ? StringComparer.OrdinalIgnoreCase : StringComparer.Ordinal);

	protected static Lock GetFileLock(string filePath) =>
		FileLocks.GetOrAdd(Path.GetFullPath(filePath), static _ => new Lock());

	public string FilePath { get; }

	public void ToFile()
	{
		lock (GetFileLock(FilePath))
		{
			File.WriteAllText(FilePath, EncodeAsJson(), Encoding.UTF8);
		}
	}

	protected abstract string EncodeAsJson();
}
