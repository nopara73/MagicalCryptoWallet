// Test-only oracle. The verifier builds this in an ignored temporary project,
// referencing the already cached Newtonsoft assembly. No PackageReference,
// managed adapter, shipping executable, wallet I/O, or network access is added.
using System;
using System.IO;
using System.Linq;
using System.Text.Json;
using Newtonsoft.Json;
using Newtonsoft.Json.Linq;

static class JsonManagedReference
{
    static JToken ExactNewtonsoft(string text)
    {
        using var reader = new JsonTextReader(new StringReader(text))
        {
            FloatParseHandling = FloatParseHandling.Decimal,
            DateParseHandling = DateParseHandling.None,
            MaxDepth = 64
        };
        do
        {
            if (!reader.Read()) throw new InvalidDataException("No JSON value");
        } while (reader.TokenType == JsonToken.Comment);
        var token = JToken.ReadFrom(reader);
        while (reader.Read())
            if (reader.TokenType != JsonToken.Comment)
                throw new InvalidDataException("Additional JSON value");
        return token;
    }

    static bool NewtonAccepts(string text)
    {
        try { _ = JToken.Parse(text); return true; }
        catch (Newtonsoft.Json.JsonException) { return false; }
    }

    static bool ManagedAccepts(string text, bool trailing = false)
    {
        try
        {
            using var document = JsonDocument.Parse(text, new JsonDocumentOptions
            {
                CommentHandling = JsonCommentHandling.Skip,
                AllowTrailingCommas = trailing
            });
            return true;
        }
        catch (System.Text.Json.JsonException) { return false; }
    }

    static int Main(string[] args)
    {
        if (args.Length != 2) throw new ArgumentException("application fixtures and owned output directory required");
        string input = Path.GetFullPath(args[0]), output = Path.GetFullPath(args[1]);
        Directory.CreateDirectory(Path.Combine(output, "newtonsoft"));
        Directory.CreateDirectory(Path.Combine(output, "system-text-json"));
        foreach (string path in Directory.GetFiles(input, "*.json").Order())
        {
            string text = File.ReadAllText(path);
            var token = ExactNewtonsoft(text);
            File.WriteAllText(Path.Combine(output, "newtonsoft", Path.GetFileName(path)), token.ToString(Formatting.None));
            using var document = JsonDocument.Parse(text, new JsonDocumentOptions
            {
                CommentHandling = JsonCommentHandling.Skip,
                MaxDepth = 64
            });
            File.WriteAllText(Path.Combine(output, "system-text-json", Path.GetFileName(path)),
                System.Text.Json.JsonSerializer.Serialize(document.RootElement));
        }
        var duplicate = JObject.Parse("{\"a\":1,\"\\u0061\":2}");
        var report = new
        {
            runtime = Environment.Version.ToString(),
            newtonsoft_assembly = typeof(JToken).Assembly.GetName().Version?.ToString(),
            default_date_token = JToken.Parse("\"2026-10-02T00:00:00+00:00\"").Type.ToString(),
            explicit_date_none_token = ExactNewtonsoft("\"2026-10-02T00:00:00+00:00\"").Type.ToString(),
            duplicate_count = duplicate.Properties().Count(),
            duplicate_value = duplicate.Value<int>("a"),
            newtonsoft_single_quotes = NewtonAccepts("{'a':1}"),
            newtonsoft_unquoted_keys = NewtonAccepts("{a:1}"),
            newtonsoft_comments = NewtonAccepts("{/*comment*/\"a\":1}"),
            newtonsoft_trailing_comma = NewtonAccepts("{\"a\":1,}"),
            managed_comments = ManagedAccepts("{/*comment*/\"a\":1}"),
            managed_trailing_comma = ManagedAccepts("{\"a\":1,}"),
            managed_trailing_comma_opt_in = ManagedAccepts("{\"a\":1,}", true),
            legacy_double_integer = (long)double.Parse("9007199254740993", System.Globalization.CultureInfo.InvariantCulture),
            exact_decimal_integer = decimal.Parse("9007199254740993", System.Globalization.CultureInfo.InvariantCulture).ToString(System.Globalization.CultureInfo.InvariantCulture)
        };
        File.WriteAllText(Path.Combine(output, "behavior.json"), System.Text.Json.JsonSerializer.Serialize(report));
        Console.WriteLine($"Managed reference: {Directory.GetFiles(input, "*.json").Length} synthetic payloads through both engines");
        return 0;
    }
}
