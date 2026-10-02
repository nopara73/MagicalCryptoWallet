// Development-only NBitcoin 10.0.13 oracle. Never linked or shipped with mcw.
using System.Text;
using NBitcoin;

var utf8 = new UTF8Encoding(false, true);
for (string? line; (line = Console.ReadLine()) is not null;)
{
    try
    {
        var words = line.Split('\t');
        var input = Convert.FromHexString(words[1]);
        var output = words[0] switch
        {
            "P" => new Script(utf8.GetString(input)).ToBytes(),
            "R" => utf8.GetBytes(new Script(input).ToString()),
            _ => throw new FormatException("Unknown test-only operation.")
        };
        Console.WriteLine("OK:" + Convert.ToHexStringLower(output));
    }
    catch (Exception)
    {
        Console.WriteLine("ERR");
    }
}
