using System;
using System.Collections;
using System.Collections.Generic;
using System.Globalization;
using System.Linq;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.TransactionBuilding;
using MagicalCryptoWallet.Rpc;

namespace MagicalCryptoWallet.Mcw.Serialization;

/// <summary>Explicit RPC mappings. Domain work stays managed; Rust owns JSON
/// grammar, primitive coercion and output. There is no serializer fallback.</summary>
public sealed class RpcObjectCodec(Network network)
{
	public object? Decode(RpcValue value, Type type, string path)
	{
		var nullable = Nullable.GetUnderlyingType(type);
		if (value.ValueKind == RpcValueKind.Null)
		{
			if (!type.IsValueType || nullable is not null) { return null; }
			throw Conversion(value, type, path);
		}
		type = nullable ?? type;
		if (type == typeof(string)) { return Text(value); }
		if (type == typeof(int))
		{
			if (value.ValueKind is not (RpcValueKind.String or RpcValueKind.Number))
			{ throw new RpcJsonException($"Error reading integer. Unexpected token: {TokenType(value)}. Path '{path}'."); }
			try { return checked((int)RpcJson.Integer(value)); }
			catch (Exception error) when (error is RpcJsonException or OverflowException)
			{
				throw new RpcJsonException(value.ValueKind == RpcValueKind.String
					? $"Could not convert string to integer: {value.ScalarText}. Path '{path}'."
					: $"Could not convert to integer: {value.NumberToken}. Path '{path}'.");
			}
		}
		if (type == typeof(long) || type == typeof(uint) || type == typeof(ulong))
		{
			try
			{
				var number = value.ValueKind is RpcValueKind.True or RpcValueKind.False
					? (value.GetBoolean() ? (Int128)1 : 0) : RpcJson.Integer(value);
				if (type == typeof(long)) { return checked((long)number); }
				if (type == typeof(uint)) { return checked((uint)number); }
				return checked((ulong)number);
			}
			catch (Exception error) when (error is RpcJsonException or OverflowException) { throw Conversion(value, type, path); }
		}
		if (type == typeof(bool))
		{
			try { return RpcJson.Boolean(value); }
			catch (RpcJsonException)
			{
				if (value.ValueKind == RpcValueKind.String)
				{ throw new RpcJsonException($"Could not convert string to boolean: {value.ScalarText}. Path '{path}'."); }
				throw Conversion(value, type, path);
			}
		}
		if (type == typeof(decimal))
		{
			if (value.ValueKind is not (RpcValueKind.String or RpcValueKind.Number))
			{ throw new RpcJsonException($"Error reading decimal. Unexpected token: {TokenType(value)}. Path '{path}'."); }
			try { return RpcJson.Decimal(value); }
			catch (RpcJsonException)
			{ throw new RpcJsonException($"Could not convert {(value.ValueKind == RpcValueKind.String ? "string to decimal" : "to decimal")}: {value.ScalarText}. Path '{path}'."); }
		}
		if (type == typeof(Guid))
		{
			if (value.ValueKind == RpcValueKind.String && Guid.TryParse(value.GetString(), out var guid)) { return guid; }
			throw Conversion(value, type, path);
		}
		if (type == typeof(uint256)) { return new uint256(Text(value)!); }
		if (type == typeof(Money))
		{
			if (TokenType(value) != "Integer")
			{ throw new RpcJsonException($"Unexpected json token type, expected is Integer and actual is {TokenType(value)}"); }
			return Money.Satoshis(checked((long)RpcJson.Integer(value)));
		}
		if (type == typeof(BitcoinAddress)) { return ParseAddress(value.ValueKind == RpcValueKind.String ? value.GetString() : null); }
		if (type == typeof(Destination))
		{
			var address = value.ValueKind == RpcValueKind.String ? value.GetString() : null;
			ArgumentException.ThrowIfNullOrWhiteSpace(address);
			return new Destination(BitcoinAddress.Create(address, network).ScriptPubKey);
		}
		if (type == typeof(OutPoint))
		{
			var hash = Field(value, "TransactionId"); var index = Field(value, "Index");
			if (hash is null || hash.ValueKind == RpcValueKind.Null) { throw new ArgumentNullException(nameof(hash)); }
			if (index is null || index.ValueKind == RpcValueKind.Null) { throw new ArgumentNullException("n"); }
			return new OutPoint(uint256.Parse(Text(hash)!), checked((uint)RpcJson.Integer(index)));
		}
		if (type == typeof(PaymentInfo))
		{
			object? Property(string name, Type propertyType)
			{
				var field = Field(value, name);
				return field is null ? null : Decode(field, propertyType, path + "." + Camel(name));
			}
			return new PaymentInfo
			{
				Sendto = (Destination)Property("Sendto", typeof(Destination))!,
				Amount = (Money)Property("Amount", typeof(Money))!,
				Label = (string)Property("Label", typeof(string))!,
				SubtractFee = (bool?)Property("SubtractFee", typeof(bool)) ?? false
			};
		}
		if (type.IsArray && value.ValueKind == RpcValueKind.Array)
		{
			var element = type.GetElementType()!; var values = value.EnumerateArray();
			var result = Array.CreateInstance(element, values.Count);
			for (var index = 0; index < values.Count; index++)
			{ result.SetValue(Decode(values[index], element, $"{path}[{index}]"), index); }
			return result;
		}
		throw Conversion(value, type, path);
	}

	public RpcValue Encode(object? value)
	{
		var active = new HashSet<object>(ReferenceEqualityComparer.Instance);
		var nodes = 0;
		return EncodeValue(value, 0);
		RpcValue EncodeValue(object? item, int depth)
		{
			if (depth >= 64 || ++nodes > 100_000) { throw new RpcJsonException("RPC result exceeds the resource limit."); }
			switch (item)
			{
				case null: return RpcValue.Null;
				case RpcValue token: return token;
				case string text: return RpcValue.String(text);
				case char character: return RpcValue.String(character.ToString());
				case bool boolean: return RpcValue.Boolean(boolean);
				case int integer: return RpcValue.Create(integer);
				case uint integer: return RpcValue.Create(integer);
				case long integer: return RpcValue.Create(integer);
				case ulong integer: return RpcValue.Create(integer);
				case short integer: return RpcValue.Create((int)integer);
				case ushort integer: return RpcValue.Create((uint)integer);
				case byte integer: return RpcValue.Create((int)integer);
				case sbyte integer: return RpcValue.Create((int)integer);
				case decimal number: return RpcValue.Create(number);
				case double number: return RpcValue.Create(number);
				case float number: return RpcValue.Create(number);
				case Guid guid: return RpcValue.Create(guid);
				case DateTimeOffset date: return RpcValue.Create(date.ToUnixTimeSeconds());
				case DateTime date: return RpcValue.Create(new DateTimeOffset(date.ToUniversalTime()).ToUnixTimeSeconds());
				case byte[] bytes: return RpcValue.String(Convert.ToHexStringLower(bytes));
				case uint256 hash: return RpcValue.String(hash.ToString());
				case BitcoinAddress address: return RpcValue.String(address.ToString());
				case OutPoint outpoint: return RpcValue.Object([("TransactionId", RpcValue.String(outpoint.Hash.ToString())), ("Index", RpcValue.Create(outpoint.N))]);
				case Destination destination: return RpcValue.String(destination.ScriptPubKey.GetDestinationAddress(network)?.ToString());
				case Money money: return RpcValue.Create(money.Satoshi);
				case FeeRate fee: return RpcValue.Create(fee.SatoshiPerByte);
				case PaymentInfo payment: return RpcValue.Object([
					("sendto", EncodeValue(payment.Sendto, depth + 1)),
					("amount", EncodeValue(payment.Amount, depth + 1)),
					("label", EncodeValue(payment.Label, depth + 1)),
					("subtractFee", RpcValue.Boolean(payment.SubtractFee))]);
				case Enum enumeration: return RpcValue.Number(Convert.ToInt64(enumeration, CultureInfo.InvariantCulture).ToString(CultureInfo.InvariantCulture));
			}
			if (!active.Add(item)) { throw new RpcJsonException("RPC result contains an unsupported reference cycle."); }
			try
			{
				if (item is IDictionary dictionary)
				{
					var members = new List<(string, RpcValue?)>();
					foreach (DictionaryEntry pair in dictionary)
					{
						if (pair.Value is { } child && active.Contains(child)) { continue; }
						if (members.Count >= 50_000) { throw new RpcJsonException("RPC result dictionary exceeds the entry limit."); }
						members.Add((Convert.ToString(pair.Key, CultureInfo.InvariantCulture)!, EncodeValue(pair.Value, depth + 1)));
					}
					return RpcValue.Object(members);
				}
				if (item is IEnumerable enumerable)
				{
					var array = new List<RpcValue>();
					foreach (var element in enumerable)
					{
						if (element is { } child && active.Contains(child)) { continue; }
						if (array.Count >= 50_000) { throw new RpcJsonException("RPC result array exceeds the entry limit."); }
						array.Add(EncodeValue(element, depth + 1));
					}
					return RpcValue.Array(array);
				}
				throw new RpcJsonException($"RPC result type '{item.GetType().Name}' has no declared mapping.");
			}
			finally { active.Remove(item); }
		}
	}

	private static RpcJsonException Conversion(RpcValue value, Type type, string path)
	{
		var text = value.ValueKind == RpcValueKind.Null ? "{null}"
			: value.ValueKind == RpcValueKind.String ? "\"" + value.ScalarText + "\"" : value.ScalarText ?? TokenType(value);
		return new($"Error converting value {text} to type '{type.FullName}'. Path '{path}'.");
	}
	internal static string TokenType(RpcValue value) => value.ValueKind switch
	{
		RpcValueKind.Number => value.NumberToken.Contains('.') || value.NumberToken.Contains('e') || value.NumberToken.Contains('E') ? "Float" : "Integer",
		RpcValueKind.True or RpcValueKind.False => "Boolean", _ => value.ValueKind.ToString()
	};
	private static string? Text(RpcValue value) => value.ValueKind switch
	{
		RpcValueKind.Null => null,
		RpcValueKind.String or RpcValueKind.Number or RpcValueKind.Date => value.ScalarText,
		RpcValueKind.True => "true", RpcValueKind.False => "false",
		_ => throw new RpcJsonException("Cannot convert a structured RPC value to a string.")
	};
	private static string Camel(string text) => char.ToLowerInvariant(text[0]) + text[1..];
	private static BitcoinAddress? ParseAddress(string? text)
	{
		if (string.IsNullOrWhiteSpace(text)) { return null; }
		text = text.Trim();
		try { return BitcoinAddress.Create(text, Network.Main); }
		catch
		{
			try { return BitcoinAddress.Create(text, Network.TestNet); }
			catch { return BitcoinAddress.Create(text, Network.RegTest); }
		}
	}
	private static RpcValue? Field(RpcValue value, string name)
	{
		if (value.TryGetProperty(name, out var exact)) { return exact; }
		return value.EnumerateObject().FirstOrDefault(member => string.Equals(member.Name, name, StringComparison.OrdinalIgnoreCase)).Value;
	}
}
