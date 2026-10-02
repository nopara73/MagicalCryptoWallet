// Test infrastructure only: use the actual production host service connection.
using System;
using System.ComponentModel;
using System.IO;
using System.Runtime.InteropServices;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Client.Application;
using Microsoft.Win32.SafeHandles;
using Xunit.Sdk;
using Xunit.v3;

[assembly: TestPipelineStartup(typeof(MagicalCryptoWallet.Tests.Infrastructure.McwManagedTestHost))]

namespace MagicalCryptoWallet.Tests.Infrastructure;

public sealed partial class McwManagedTestHost : ITestPipelineStartup
{
	private static ManagedApplicationHost? _host;

	// Called by the existing test module initializer before runner output starts.
	public static void Initialize()
	{
		if (Environment.GetEnvironmentVariable("MCW_HOSTED") == "1")
		{
			// MTP can open the OS stdout directly. Keep IPC on its original pipe and
			// route runner output to stderr before the runner creates its writers.
			Stream? output = null;
			Stream? input = null;
			try
			{
				output = ReserveBridgeOutput();
				input = Console.OpenStandardInput();
				_host = ManagedApplicationHost.Connect(input, output);
				input = null;
				output = null;
			}
			finally
			{
				input?.Dispose();
				output?.Dispose();
			}
		}
	}

	private static Stream ReserveBridgeOutput()
	{
		if (OperatingSystem.IsWindows())
		{
			var output = Console.OpenStandardOutput();
			if (SetStdHandle(-11, GetStdHandle(-12)) == 0)
			{
				int error = Marshal.GetLastPInvokeError();
				output.Dispose();
				throw new Win32Exception(error);
			}
			return output;
		}
		int descriptor = OperatingSystem.IsMacOS() ? MacDup(1) : LinuxDup(1);
		if (descriptor < 0) { throw new Win32Exception(Marshal.GetLastPInvokeError()); }
#pragma warning disable CA2000 // Returned FileStream owns the duplicated descriptor; the catch closes it if construction fails.
		var handle = new SafeFileHandle((nint)descriptor, ownsHandle: true);
#pragma warning restore CA2000
		try
		{
			int redirected = OperatingSystem.IsMacOS() ? MacDup2(2, 1) : LinuxDup2(2, 1);
			if (redirected < 0) { throw new Win32Exception(Marshal.GetLastPInvokeError()); }
			return new FileStream(handle, FileAccess.Write);
		}
		catch
		{
			handle.Dispose();
			throw;
		}
	}

	[LibraryImport("kernel32.dll", SetLastError = true)]
	private static partial nint GetStdHandle(int handle);
	[LibraryImport("kernel32.dll", SetLastError = true)]
	private static partial int SetStdHandle(int handle, nint value);
	[LibraryImport("libc.so.6", EntryPoint = "dup", SetLastError = true)]
	private static partial int LinuxDup(int descriptor);
	[LibraryImport("libc.so.6", EntryPoint = "dup2", SetLastError = true)]
	private static partial int LinuxDup2(int oldDescriptor, int newDescriptor);
	[LibraryImport("libSystem.B.dylib", EntryPoint = "dup", SetLastError = true)]
	private static partial int MacDup(int descriptor);
	[LibraryImport("libSystem.B.dylib", EntryPoint = "dup2", SetLastError = true)]
	private static partial int MacDup2(int oldDescriptor, int newDescriptor);

	public ValueTask StartAsync(IMessageSink diagnosticMessageSink)
	{
		if (_host is null)
		{
			throw new InvalidOperationException("Run the managed suite through mcw with psbt_metadata_suite_verify.ps1; the real application service connection is required.");
		}
		Console.Error.WriteLine("MCW_MANAGED_TEST_HOST_CONNECTED services=production bridge=stdio");
		return default;
	}

	public ValueTask StopAsync()
	{
		Interlocked.Exchange(ref _host, null)?.Dispose();
		Console.Error.WriteLine("MCW_MANAGED_TEST_HOST_CLOSED");
		return default;
	}
}
