using System.IO;
using System.Text;
using MagicalCryptoWallet.Io;
public static class CandidateInvoker
{
    public static void Text(string path, string text, Encoding encoding) => File.SafelyWriteAllText(path, text, encoding);
    public static void Bytes(string path, byte[] content) => File.SafelyWriteAllBytes(path, content);
}
