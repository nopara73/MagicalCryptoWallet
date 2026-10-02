using System;
using System.Collections.Generic;
using System.IO;
using System.Runtime.InteropServices;

// Test-only closure of the synthetic child's private output pipe. Keep stdin
// open so the child can observe graceful native shutdown after output EOF.
internal static class ProbePipe
{
    [DllImport("kernel32.dll", SetLastError = true)]
    private static extern IntPtr GetStdHandle(int handle);

    [DllImport("kernel32.dll", SetLastError = true)]
    [return: MarshalAs(UnmanagedType.Bool)]
    private static extern bool CloseHandle(IntPtr handle);

    [DllImport("libc", EntryPoint = "close", SetLastError = true)]
    private static extern int Close(int descriptor);

    // The retained .NET runtime normalizes Linux/Darwin stat layouts and symbol
    // aliases. This binding is not an import of the Rust shipping executable.
    // Layout: dotnet/runtime v10.0.12, System.Native/pal_io.h FileStatus.
    [StructLayout(LayoutKind.Explicit, Size = 120)]
    private struct FileStatus
    {
        [FieldOffset(4)] public int Mode;
        [FieldOffset(88)] public long Device;
        [FieldOffset(104)] public long Inode;
    }
    [DllImport("System.Native", EntryPoint = "SystemNative_FStat", SetLastError = true)]
    private static extern int Stat(IntPtr descriptor, out FileStatus status);

    [DllImport("libc", EntryPoint = "getdtablesize")]
    private static extern int DescriptorLimit();

    private static IEnumerable<int> OpenDescriptors()
    {
        // Linux runners can allow over a million descriptors. Enumerate the
        // open set so fixture closure does not delay EOF until work completes.
        if (OperatingSystem.IsLinux() && Directory.Exists("/proc/self/fd"))
        {
            foreach (var path in Directory.GetFiles("/proc/self/fd"))
            {
                if (int.TryParse(Path.GetFileName(path), out var descriptor) && descriptor >= 3)
                { yield return descriptor; }
            }
            yield break;
        }
        var limit = DescriptorLimit();
        for (var descriptor = 3; descriptor < limit; descriptor++) { yield return descriptor; }
    }

    public static void CloseOutput()
    {
        if (!OperatingSystem.IsWindows())
        {
            // CoreCLR duplicates standard handles before managed entry. Close
            // only aliases of the same output pipe, keeping fd 1 open until
            // the scan is complete so its inode cannot be reused mid-scan.
            if (Stat(new IntPtr(1), out var identity) != 0 || (identity.Mode & 0xF000) != 0x1000 || identity.Inode == 0)
            { throw new IOException("Could not identify synthetic child output pipe."); }
            foreach (var descriptor in OpenDescriptors())
            {
                if (Stat(new IntPtr(descriptor), out var candidate) == 0 && (candidate.Mode & 0xF000) == 0x1000
                    && candidate.Device == identity.Device && candidate.Inode == identity.Inode
                    && Close(descriptor) != 0)
                {
                    throw new IOException("Could not close synthetic child output copy.");
                }
            }
        }
        var success = OperatingSystem.IsWindows() ? CloseHandle(GetStdHandle(-11)) : Close(1) == 0;
        if (!success) { throw new IOException("Could not close synthetic child output."); }
    }
}
