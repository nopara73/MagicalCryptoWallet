using System.IO;
namespace MagicalCryptoWallet.Helpers;
public static class IoHelpers {
 public static void EnsureContainingDirectoryExists(string fileNameOrPath) { string fullPath=Path.GetFullPath(fileNameOrPath); string? dir=Path.GetDirectoryName(fullPath); EnsureDirectoryExists(dir); }
 public static void EnsureDirectoryExists(string? dir) { if(!string.IsNullOrWhiteSpace(dir) && !Directory.Exists(dir)) { Directory.CreateDirectory(dir); } }
}