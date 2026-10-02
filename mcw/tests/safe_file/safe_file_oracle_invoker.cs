using System.IO;
using System.Text;
using SafeFileOracle.Io;
public static class OracleInvoker
{
    public static void Text(string path, string text, Encoding encoding) => File.SafelyWriteAllText(path, text, encoding);
    public static void Bytes(string path, byte[] content) => File.SafelyWriteAllBytes(path, content);
}
