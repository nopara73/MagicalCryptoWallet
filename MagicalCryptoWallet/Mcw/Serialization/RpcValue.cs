using System;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;

namespace MagicalCryptoWallet.Mcw.Serialization;

public enum RpcValueKind { Null, False, True, Number, String, Array, Object, Date }
public readonly record struct RpcJsonProperty(string Name, RpcValue Value);

/// <summary>Immutable typed values. JSON grammar and output are owned by Rust.</summary>
public sealed class RpcValue
{
	private readonly string? _text;
	private readonly IReadOnlyList<RpcValue>? _array;
	private readonly IReadOnlyList<RpcJsonProperty>? _object;
	private RpcValue(RpcValueKind kind, string? text = null,
		IReadOnlyList<RpcValue>? array = null, IReadOnlyList<RpcJsonProperty>? members = null)
	{ ValueKind = kind; _text = text; _array = array; _object = members; }

	public RpcValueKind ValueKind { get; }
	public static RpcValue Null { get; } = new(RpcValueKind.Null);
	public static RpcValue String(string? text) => text is null ? Null : new(RpcValueKind.String, text);
	internal static RpcValue Date(string text) => new(RpcValueKind.Date, text);
	internal string? ScalarText => _text;
	public static RpcValue Number(string token) => new(RpcValueKind.Number, token ?? throw new ArgumentNullException(nameof(token)));
	public static RpcValue Boolean(bool value) => new(value ? RpcValueKind.True : RpcValueKind.False);
	public static RpcValue Array(IEnumerable<RpcValue?> values) => new(RpcValueKind.Array,
		array: System.Array.AsReadOnly(values.Select(value => value ?? Null).ToArray()));
	public static RpcValue Object(IEnumerable<(string Name, RpcValue? Value)> members)
	{
		var names = new HashSet<string>(StringComparer.Ordinal);
		var result = new List<RpcJsonProperty>();
		foreach (var (name, value) in members)
		{
			if (!names.Add(name)) { throw new RpcJsonException("Duplicate JSON object key."); }
			result.Add(new(name, value ?? Null));
		}
		return new(RpcValueKind.Object, members: result.AsReadOnly());
	}
	public static RpcValue Create(string? value) => String(value);
	public static RpcValue Create(bool value) => Boolean(value);
	public static RpcValue Create(int value) => Number(value.ToString(CultureInfo.InvariantCulture));
	public static RpcValue Create(uint value) => Number(value.ToString(CultureInfo.InvariantCulture));
	public static RpcValue Create(long value) => Number(value.ToString(CultureInfo.InvariantCulture));
	public static RpcValue Create(ulong value) => Number(value.ToString(CultureInfo.InvariantCulture));
	public static RpcValue Create(decimal value) => Number(DecimalToken(value));
	private static string DecimalToken(decimal value) { var text = value.ToString(CultureInfo.InvariantCulture); return text.Contains('.') ? text : text + ".0"; }
	public static RpcValue Create(double value)
	{
		if (!double.IsFinite(value)) { return String(value.ToString(CultureInfo.InvariantCulture)); }
		var text = value.ToString("R", CultureInfo.InvariantCulture);
		return Number(text.Contains('.') || text.Contains('E') ? text : text + ".0");
	}
	public static RpcValue Create(float value)
	{
		if (!float.IsFinite(value)) { return String(value.ToString(CultureInfo.InvariantCulture)); }
		var text = value.ToString("R", CultureInfo.InvariantCulture);
		return Number(text.Contains('.') || text.Contains('E') ? text : text + ".0");
	}
	public static RpcValue Create(Guid value) => String(value.ToString("D"));
	public static RpcValue Create(DateTimeOffset value) => Create(value.ToUnixTimeSeconds());
	public static implicit operator RpcValue(string? value) => String(value);

	public string? GetString() => ValueKind is RpcValueKind.String or RpcValueKind.Null
		? _text : throw new RpcJsonException("The JSON value is not a string.");
	public bool GetBoolean() => ValueKind switch
	{ RpcValueKind.True => true, RpcValueKind.False => false, _ => throw new RpcJsonException("The JSON value is not a boolean.") };
	public string NumberToken => ValueKind == RpcValueKind.Number ? _text! : throw new RpcJsonException("The JSON value is not a number.");
	public int GetInt32() => checked((int)RpcJson.Integer(this));
	public uint GetUInt32() => checked((uint)RpcJson.Integer(this));
	public long GetInt64() => checked((long)RpcJson.Integer(this));
	public ulong GetUInt64() => checked((ulong)RpcJson.Integer(this));
	public decimal GetDecimal() => RpcJson.Decimal(this);
	public int GetArrayLength() => Items.Count;
	public RpcValue this[int index] => Items[index];
	public IReadOnlyList<RpcValue> EnumerateArray() => Items;
	public IReadOnlyList<RpcJsonProperty> EnumerateObject() => Members;
	private IReadOnlyList<RpcValue> Items => _array ?? throw new RpcJsonException("The JSON value is not an array.");
	private IReadOnlyList<RpcJsonProperty> Members => _object ?? throw new RpcJsonException("The JSON value is not an object.");
	public bool TryGetProperty(string name, out RpcValue value)
	{
		foreach (var member in Members)
		{ if (member.Name == name) { value = member.Value; return true; } }
		value = Null; return false;
	}
	public RpcValue GetProperty(string name) => TryGetProperty(name, out var value)
		? value : throw new RpcJsonException("Required JSON property is missing.");

}

public sealed class RpcJsonException : FormatException
{
	public RpcJsonException(string message) : base(message) { }
	public RpcJsonException(string message, Exception inner) : base(message, inner) { }
}
