using System;
using System.Collections.Generic;
using System.IO;
using System.Text;

namespace MagicalCryptoWallet.Mcw.Serialization;

internal static class RpcTokens
{
	private const int MaxNodes = 100_000, MaxEntries = 50_000, MaxString = 1024 * 1024, MaxNumber = 4096;
	public static byte[] Encode(RpcValue value)
	{
		using var output = new MemoryStream(); using var writer = new BinaryWriter(output, RpcJson.Utf8, leaveOpen: true);
		int nodes = 0, strings = 0;
		Write(value, 0);
		return output.ToArray();
		void Text(string text, bool number)
		{
			int count;
			try { count = RpcJson.Utf8.GetByteCount(text); }
			catch (EncoderFallbackException error) { throw new RpcJsonException("Typed JSON contains an unpaired surrogate.", error); }
			if (count > (number ? MaxNumber : MaxString) || (!number && (strings += count) > RpcJson.MaxJsonBytes)
				|| output.Length + 4 + count > RpcJson.MaxTransferBytes) { throw new RpcJsonException("Typed JSON resource limit exceeded."); }
			writer.Write(count); writer.Write(RpcJson.Utf8.GetBytes(text));
		}
		void Node() { if (++nodes > MaxNodes || output.Length >= RpcJson.MaxTransferBytes) { throw new RpcJsonException("Typed JSON resource limit exceeded."); } }
		void Write(RpcValue item, int depth)
		{
			Node(); writer.Write((byte)item.ValueKind);
			switch (item.ValueKind)
			{
				case RpcValueKind.Number: Text(item.NumberToken, true); break;
				case RpcValueKind.String: Text(item.GetString()!, false); break;
				case RpcValueKind.Date: throw new RpcJsonException("Parsed date tokens cannot be written as RPC results.");
				case RpcValueKind.Array:
					var array = item.EnumerateArray(); Container(depth, array.Count); writer.Write(array.Count);
					foreach (var child in array) { Write(child, depth + 1); } break;
				case RpcValueKind.Object:
					var members = item.EnumerateObject(); Container(depth, members.Count); writer.Write(members.Count);
					foreach (var member in members) { Node(); Text(member.Name, false); Write(member.Value, depth + 1); } break;
			}
		}
	}
	public static RpcValue Decode(byte[] bytes)
	{
		if (bytes.Length > RpcJson.MaxTransferBytes) { throw new RpcJsonException("Typed JSON input exceeds the limit."); }
		using var input = new MemoryStream(bytes, writable: false); using var reader = new BinaryReader(input, RpcJson.Utf8);
		int nodes = 0, strings = 0;
		try
		{
			var result = Read(0);
			if (input.Position != input.Length) { throw new RpcJsonException("Trailing typed JSON bytes."); }
			return result;
		}
		catch (Exception error) when (error is EndOfStreamException or DecoderFallbackException or ArgumentException)
		{ throw new RpcJsonException("Invalid native typed JSON response.", error); }
		string Text(bool number)
		{
			var count = reader.ReadInt32();
			if (count < 0 || count > (number ? MaxNumber : MaxString) || count > input.Length - input.Position
				|| (!number && (strings += count) > RpcJson.MaxJsonBytes)) { throw new RpcJsonException("Invalid typed JSON string length."); }
			return RpcJson.Utf8.GetString(reader.ReadBytes(count));
		}
		RpcValue Read(int depth)
		{
			if (++nodes > MaxNodes) { throw new RpcJsonException("Typed JSON node limit exceeded."); }
			switch ((RpcValueKind)reader.ReadByte())
			{
				case RpcValueKind.Null: return RpcValue.Null;
				case RpcValueKind.False: return RpcValue.Boolean(false);
				case RpcValueKind.True: return RpcValue.Boolean(true);
				case RpcValueKind.Number: return RpcValue.Number(Text(true));
				case RpcValueKind.String: return RpcValue.String(Text(false));
				case RpcValueKind.Date: return RpcValue.Date(Text(false));
				case RpcValueKind.Array:
					var count = reader.ReadInt32(); Container(depth, count);
					if (count > input.Length - input.Position || count > MaxNodes - nodes) { throw new RpcJsonException("Invalid typed JSON array length."); }
					var array = new RpcValue[count];
					for (var i = 0; i < count; i++) { array[i] = Read(depth + 1); }
					return RpcValue.Array(array);
				case RpcValueKind.Object:
					var length = reader.ReadInt32(); Container(depth, length);
					if (length > input.Length - input.Position || length > (MaxNodes - nodes) / 2) { throw new RpcJsonException("Invalid typed JSON object length."); }
					var members = new (string, RpcValue?)[length];
					for (var i = 0; i < length; i++)
					{
						if (++nodes > MaxNodes) { throw new RpcJsonException("Typed JSON node limit exceeded."); }
						members[i] = (Text(false), Read(depth + 1));
					}
					return RpcValue.Object(members);
				default: throw new RpcJsonException("Invalid typed JSON tag.");
			}
		}
	}
	private static void Container(int depth, int count)
	{ if (depth >= 64 || count < 0 || count > MaxEntries) { throw new RpcJsonException("Typed JSON container limit exceeded."); } }
}
