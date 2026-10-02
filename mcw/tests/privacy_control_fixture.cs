// Independent fixture generator. Compile with the original managed reader from
// f965ae7b16fbc936582611070cb4e77813dce52b, not the migrated adapter. The
// generated TSVs are committed so ordinary Rust tests need neither .NET nor Git.
using System.Globalization;
using System.IO.Pipelines;
using System.Text;
using MagicalCryptoWallet.Tor.Control;

var directory = Path.GetFullPath(args[0]);
Directory.CreateDirectory(directory);
var vectors = new (string Name, byte[] Wire)[]
{
    ("ok", "250 OK\r\n"u8.ToArray()),
    ("bare-status", "250\r\n"u8.ToArray()),
    ("empty-text", "250 \r\n"u8.ToArray()),
    ("unknown-separator", "250?compat\r\n"u8.ToArray()),
    ("event", "650 CIRC 1000 EXTENDED moria1,moria2\r\n"u8.ToArray()),
    ("protocolinfo", "250-PROTOCOLINFO 1\r\n250-AUTH METHODS=SAFECOOKIE COOKIEFILE=\"synthetic\"\r\n250-VERSION Tor=\"0.4.8.1\"\r\n250 OK\r\n"u8.ToArray()),
    ("literal-escapes", "250-PROTOCOLINFO 1\r\n250-VERSION Tor=\\\"0.4.3.5\\\" \\n\\t\\r\r\n250 OK\r\n"u8.ToArray()),
    ("getconf", "250-SOCKSPORT=38150\r\n250 ORPORT=0\r\n"u8.ToArray()),
    ("circuit-status", "250+circuit-status=\r\n1 BUILT $synthetic PURPOSE=GENERAL\r\n.\r\n250 OK\r\n"u8.ToArray()),
    ("empty-data", "250+circuit-status=\r\n.\r\n250 OK\r\n"u8.ToArray()),
    ("data-blank-lines", "250+data=\r\n\r\na\r\n\r\n.\r\n250 OK\r\n"u8.ToArray()),
    ("stuffed-dots", "250+data=\r\n..\r\n..payload\r\n.\r\n250 OK\r\n"u8.ToArray()),
    ("data-following-line", "250+data=\r\n.\r\n650 NOTICE retained\r\n"u8.ToArray()),
    ("minus-short-lines", "250-start\r\na\r\nxyz\r\nlongbody\r\n250 OK\r\n"u8.ToArray()),
    ("minus-blank-lines", "250-start\r\n\r\n250-next\r\n250 OK\r\n"u8.ToArray()),
    ("different-terminal-status", "250-start\r\n551 terminal\r\n"u8.ToArray()),
    ("plus-in-minus", "250-start\r\n250+body\r\n250 OK\r\n"u8.ToArray()),
    ("status-zero", "000 arbitrary\r\n"u8.ToArray()),
    ("status-unknown", "999 arbitrary\r\n"u8.ToArray()),
    ("status-sign", "+25 compat\r\n"u8.ToArray()),
    ("status-negative", "-25 compat\r\n"u8.ToArray()),
    ("status-whitespace", " 25 compat\r\n"u8.ToArray()),
    ("ascii-replacement", [50,53,48,32,128,255,0,13,10]),
    ("synthetic-authchallenge", Encoding.ASCII.GetBytes("250 AUTHCHALLENGE SERVERHASH=" + new string('A', 64) + " SERVERNONCE=" + new string('B', 64) + "\r\n")),
};
using (var output = new StreamWriter(Path.Combine(directory, "replies.tsv"), false, new UTF8Encoding(false)))
{
    output.WriteLine("# Original C# reader at f965ae7b16; name wire-hex status lines-hex-comma-separated provenance");
    foreach (var (name, wire) in vectors)
    {
        var pipe = new Pipe(new PipeOptions(pauseWriterThreshold: 0));
        await pipe.Writer.WriteAsync(wire);
        await pipe.Writer.CompleteAsync();
        using var stop = new CancellationTokenSource(TimeSpan.FromSeconds(5));
        var result = await TorControlReplyReader.ReadReplyAsync(pipe.Reader, stop.Token);
        await pipe.Reader.CompleteAsync();
        output.WriteLine($"{name}\t{Convert.ToHexString(wire)}\t{(int)result.StatusCode}\t{string.Join(',', result.ResponseLines.Select(s => Convert.ToHexString(Encoding.ASCII.GetBytes(s))))}\toracle");
    }
}
using (var output = new StreamWriter(Path.Combine(directory, "status.tsv"), false, new UTF8Encoding(false)))
{
    output.WriteLine("# .NET 10 invariant Integer TryParse oracle; prefix-hex parsed-number-or-x");
    byte[] alphabet = [0,9,11,12,32,43,45,48,49,50,51,52,53,54,55,56,57,63,127,255];
    foreach (var a in alphabet)
    foreach (var b in alphabet)
    foreach (var c in alphabet)
    {
        byte[] prefix = [a,b,c];
        var text = Encoding.ASCII.GetString(prefix);
        var parsed = int.TryParse(text, NumberStyles.Integer, CultureInfo.InvariantCulture, out var value);
        output.WriteLine($"{Convert.ToHexString(prefix)}\t{(parsed ? value.ToString(CultureInfo.InvariantCulture) : "x")}");
    }
}
Console.WriteLine($"Generated {vectors.Length} replies and 8000 status-prefix oracle cases.");
