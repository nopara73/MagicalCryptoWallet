using System;
using System.IO;
using System.Text;
using System.Linq;
using NBitcoin;
using MagicalCryptoWallet.Helpers;

namespace MagicalCryptoWallet.Client;

/// <summary>Explicit installation context used by startup registrations and relaunches.</summary>
public sealed record StartupLaunch(string Executable, string DataDirectory, Network Network)
{
	public const string SilentArgument = "startsilent";
	public string[] Arguments(bool silent = true) => silent
		? [SilentArgument, "--datadir=" + Path.GetFullPath(DataDirectory), "--network=" + Network.Name]
		: ["--datadir=" + Path.GetFullPath(DataDirectory), "--network=" + Network.Name];
	public string WindowsCommandLine => string.Join(" ", new[] { Executable }.Concat(Arguments()).Select(QuoteWindows));
	public string DesktopCommandLine => string.Join(" ", new[] { Executable }.Concat(Arguments()).Select(QuoteDesktop));
	public static StartupLaunch Default => new(EnvironmentHelpers.GetExecutablePath(), Configuration.Config.DataDir, Network.Main);
	private static string QuoteWindows(string value)
	{
		var result = new StringBuilder("\"");
		int slashes = 0;
		foreach (char character in value)
		{
			if (character == '\\') { slashes++; continue; }
			result.Append('\\', character == '"' ? slashes * 2 + 1 : slashes);
			result.Append(character);
			slashes = 0;
		}
		return result.Append('\\', slashes * 2).Append('"').ToString();
	}
	private static string QuoteDesktop(string value) => "\"" + value.Replace("\\", "\\\\\\\\", StringComparison.Ordinal).Replace("\"", "\\\\\\\"", StringComparison.Ordinal).Replace("$", "\\\\$", StringComparison.Ordinal).Replace("`", "\\\\`", StringComparison.Ordinal).Replace("%", "%%", StringComparison.Ordinal) + "\"";
}
