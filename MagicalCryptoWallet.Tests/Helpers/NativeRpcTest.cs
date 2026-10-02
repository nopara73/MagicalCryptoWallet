using System.Diagnostics;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Tests.Helpers;

/// <summary>Runs existing RPC regressions through the actual mcw host and the
/// production managed adapter. There is no test JSON parser or in-process fake.</summary>
internal static class NativeRpcTest
{
	public static async Task<string> GetResponseAsync(string request)
	{
		var root = new DirectoryInfo(AppContext.BaseDirectory);
		while (root is not null && !File.Exists(Path.Combine(root.FullName, "Directory.Build.props"))) { root = root.Parent; }
		if (root is null) { throw new DirectoryNotFoundException("Cannot locate the RPC probe project."); }
		var probeRoot = Path.Combine(root.FullName, "Contrib", "Mcw", "RpcProbe", "bin");
		var suffix = OperatingSystem.IsWindows() ? ".exe" : "";
		var expectedConfiguration = AppContext.BaseDirectory.Contains(Path.DirectorySeparatorChar + "Release" + Path.DirectorySeparatorChar, StringComparison.Ordinal) ? "Release" : "Debug";
		var candidates = Directory.GetFiles(Path.Combine(probeRoot, expectedConfiguration), "mcw" + suffix, SearchOption.AllDirectories);
		if (candidates.Length != 1) { throw new InvalidOperationException("Build the RPC probe with exactly one target runtime for this test run."); }
		var native = candidates[0];
		var source = Path.GetDirectoryName(native)!;
		var temporary = Path.Combine(root.FullName, ".artifacts", "rpc-tests", Guid.NewGuid().ToString("N"));
		Directory.CreateDirectory(temporary);
		try
		{
			foreach (var file in Directory.GetFiles(source)) { File.Copy(file, Path.Combine(temporary, Path.GetFileName(file))); }
			File.Copy(Path.Combine(temporary, "RpcProbe" + suffix), Path.Combine(temporary, "magicalcryptowalletd" + suffix));
			var fixture = Path.Combine(temporary, "request.tsv");
			var report = Path.Combine(temporary, "report");
			await File.WriteAllTextAsync(fixture, Convert.ToBase64String(Encoding.UTF8.GetBytes(request)) + "\t\ttestable\n");
			var start = new ProcessStartInfo(Path.Combine(temporary, "mcw" + suffix)) { UseShellExecute = false, RedirectStandardError = true, RedirectStandardOutput = true };
			start.ArgumentList.Add("daemon"); start.ArgumentList.Add(fixture); start.ArgumentList.Add(report); start.ArgumentList.Add("single");
			using var child = Process.Start(start) ?? throw new IOException("Cannot launch the native RPC test host.");
			var errors = child.StandardError.ReadToEndAsync();
			var output = child.StandardOutput.ReadToEndAsync();
			using var timeout = new CancellationTokenSource(TimeSpan.FromSeconds(30));
			try { await child.WaitForExitAsync(timeout.Token); }
			catch (OperationCanceledException) { child.Kill(entireProcessTree: true); await child.WaitForExitAsync(); throw; }
			await output;
			if (child.ExitCode != 0) { throw new IOException("Native RPC test failed: " + await errors); }
			var columns = (await File.ReadAllLinesAsync(report + ".tsv"))[0].Split('\t');
			return Encoding.UTF8.GetString(Convert.FromBase64String(columns[1]));
		}
		finally
		{
			// Only this test's freshly created directory and copied files are removed.
			foreach (var file in Directory.GetFiles(temporary)) { File.Delete(file); }
			Directory.Delete(temporary);
		}
	}
}
