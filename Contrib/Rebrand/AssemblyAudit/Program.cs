using System.Reflection.Metadata;
using System.Reflection.Metadata.Ecma335;
using System.Reflection.PortableExecutable;
using System.Text.Json;
using System.Text.RegularExpressions;

var policy = JsonDocument.Parse(File.ReadAllText(args[0]));
var forbidden = new Regex(policy.RootElement.GetProperty("forbidden_pattern").GetString()!, RegexOptions.IgnoreCase);
var removedControls = new Regex(policy.RootElement.GetProperty("removed_coin_control_pattern").GetString()!, RegexOptions.IgnoreCase);
int assemblies = 0, symbols = 0, failures = 0;
foreach (string folder in args.Skip(1))
foreach (string file in Directory.EnumerateFiles(folder, "MagicalCryptoWallet*.dll", SearchOption.AllDirectories))
{
    if (Path.GetFileName(file).Contains("Tests", StringComparison.Ordinal)) continue;
    using var stream = File.OpenRead(file);
#pragma warning disable CA2000 // The PEReader is disposed by this using declaration; metadata views do not own it.
    using var pe = new PEReader(stream, PEStreamOptions.LeaveOpen);
#pragma warning restore CA2000
    if (!pe.HasMetadata) continue;
    assemblies++;
    var reader = pe.GetMetadataReader();
    Check(reader.GetString(reader.GetAssemblyDefinition().Name));
    foreach (var handle in reader.AssemblyReferences) Check(reader.GetString(reader.GetAssemblyReference(handle).Name));
    foreach (var handle in reader.TypeDefinitions)
    {
        var type = reader.GetTypeDefinition(handle);
        Check(reader.GetString(type.Name)); Check(reader.GetString(type.Namespace));
    }
    foreach (var handle in reader.TypeReferences)
    {
        var type = reader.GetTypeReference(handle);
        Check(reader.GetString(type.Name)); Check(reader.GetString(type.Namespace));
    }
    foreach (var handle in reader.MethodDefinitions)
    {
        var method = reader.GetMethodDefinition(handle); Check(reader.GetString(method.Name));
        foreach (var param in method.GetParameters()) Check(reader.GetString(reader.GetParameter(param).Name));
    }
    foreach (var handle in reader.FieldDefinitions) Check(reader.GetString(reader.GetFieldDefinition(handle).Name));
    foreach (var handle in reader.PropertyDefinitions) Check(reader.GetString(reader.GetPropertyDefinition(handle).Name));
    foreach (var handle in reader.EventDefinitions) Check(reader.GetString(reader.GetEventDefinition(handle).Name));
    foreach (var handle in reader.ManifestResources) Check(reader.GetString(reader.GetManifestResource(handle).Name));
    // Brand-bearing literal strings and assembly attributes live in these heaps.
    var userString = MetadataTokens.UserStringHandle(1);
    while (!userString.IsNil)
    {
        Check(reader.GetUserString(userString));
        userString = reader.GetNextHandle(userString);
    }
    foreach (var handle in reader.CustomAttributes)
    {
        var attribute = reader.GetCustomAttribute(handle);
        if (attribute.Constructor.Kind != HandleKind.MemberReference) continue;
        var member = reader.GetMemberReference((MemberReferenceHandle)attribute.Constructor);
        if (member.Parent.Kind != HandleKind.TypeReference) continue;
        string name = reader.GetString(reader.GetTypeReference((TypeReferenceHandle)member.Parent).Name);
        if (name is not ("AssemblyProductAttribute" or "AssemblyCompanyAttribute" or "AssemblyTitleAttribute" or "AssemblyMetadataAttribute")) continue;
        var blob = reader.GetBlobReader(attribute.Value);
        if (blob.ReadUInt16() != 1) throw new InvalidDataException("Invalid assembly attribute.");
        Check(blob.ReadSerializedString() ?? "");
        if (name == "AssemblyMetadataAttribute") Check(blob.ReadSerializedString() ?? "");
    }
    foreach (var entry in pe.ReadDebugDirectory())
    {
        if (entry.Type != DebugDirectoryEntryType.EmbeddedPortablePdb) continue;
        using var provider = pe.ReadEmbeddedPortablePdbDebugDirectoryData(entry);
        CheckDocuments(provider.GetMetadataReader());
    }
    string pdb = Path.ChangeExtension(file, ".pdb");
    if (File.Exists(pdb))
    {
        using var pdbStream = File.OpenRead(pdb);
        using var provider = MetadataReaderProvider.FromPortablePdbStream(pdbStream);
        CheckDocuments(provider.GetMetadataReader());
    }
    void CheckDocuments(MetadataReader pdbReader)
    {
        symbols++;
        foreach (var handle in pdbReader.Documents) Check(pdbReader.GetString(pdbReader.GetDocument(handle).Name));
    }
    void Check(string value)
    {
        if (!forbidden.IsMatch(value) && !removedControls.IsMatch(value)) return;
        Console.Error.WriteLine($"Stale assembly/resource/symbol identity: {Path.GetFileName(file)}: {value}");
        failures++;
    }
}
Console.WriteLine(JsonSerializer.Serialize(new { assemblies, symbols, failures }));
return failures == 0 && assemblies > 0 ? 0 : 1;
