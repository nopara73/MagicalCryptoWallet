using System;
using System.Buffers.Binary;
using System.Collections.Generic;
using System.IO;
using System.Text;
using System.Threading;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Mcw.NativeUi;

[Flags]
public enum MarkdownStyle : byte { None = 0, Bold = 1, Italic = 2, Code = 4, Strike = 8 }
public enum MarkdownBlockKind : byte { Paragraph, Heading, ListItem, Code, Quote, Rule }
public sealed record MarkdownRun(string Text, MarkdownStyle Style, string? Link, string? Title);
public sealed record MarkdownBlock(MarkdownBlockKind Kind, byte Level, byte Depth, string Marker, IReadOnlyList<MarkdownRun> Runs);
public sealed record MarkdownDocument(IReadOnlyList<MarkdownBlock> Blocks);

/// <summary>Typed presentation adapter over the existing mcw connection. It never parses Markdown.</summary>
public static class MarkdownPresentation
{
	public const ushort Operation = 0x1100;
	public const byte Schema = 1;
	public const int MaximumInputBytes = 262_144;
	public const int MaximumOutputBytes = 1_000_000;
	private static readonly UTF8Encoding Utf8 = new(false, true);

	public static async Task<MarkdownDocument> ParseAsync(string source, CancellationToken cancellationToken = default)
	{
		ArgumentNullException.ThrowIfNull(source);
		cancellationToken.ThrowIfCancellationRequested();
		if (source.IndexOf('\0') >= 0 || Utf8.GetByteCount(source) > MaximumInputBytes)
		{
			throw new ArgumentException("Invalid release-highlights Markdown input.", nameof(source));
		}
		var payload = new byte[1 + Utf8.GetByteCount(source)];
		payload[0] = Schema;
		Utf8.GetBytes(source.AsSpan(), payload.AsSpan(1));
		var response = await McwApplicationServices.Current.RequestAsync(Operation, payload, cancellationToken).ConfigureAwait(false);
		cancellationToken.ThrowIfCancellationRequested();
		return Decode(response);
	}

	public static MarkdownDocument Decode(ReadOnlySpan<byte> payload)
	{
		if (payload.Length > MaximumOutputBytes) { throw Invalid(); }
		var reader = new Reader(payload);
		if (reader.Byte() != Schema) { throw Invalid(); }
		var count = reader.Count(4096);
		var blocks = new MarkdownBlock[count];
		var remainingRuns = 32_768;
		for (var i = 0; i < count; i++)
		{
			var kind = (MarkdownBlockKind)reader.Byte();
			var level = reader.Byte();
			var depth = reader.Byte();
			if (kind > MarkdownBlockKind.Rule || (kind == MarkdownBlockKind.Heading ? level is < 1 or > 6 : level != 0)
				|| (kind is MarkdownBlockKind.ListItem or MarkdownBlockKind.Quote ? depth > 32 : depth != 0)) { throw Invalid(); }
			var marker = reader.String();
			var runCount = reader.Count(remainingRuns);
			remainingRuns -= runCount;
			if (kind == MarkdownBlockKind.Rule && runCount != 0) { throw Invalid(); }
			var runs = new MarkdownRun[runCount];
			for (var j = 0; j < runCount; j++)
			{
				var style = (MarkdownStyle)reader.Byte();
				if (((byte)style & ~15) != 0) { throw Invalid(); }
				var text = reader.String();
				var link = reader.String();
				var title = reader.String();
				if (link.Length != 0 && !IsSafeLink(link)) { throw Invalid(); }
				runs[j] = new MarkdownRun(text, style, link.Length == 0 ? null : link, title.Length == 0 ? null : title);
			}
			blocks[i] = new MarkdownBlock(kind, level, depth, marker, runs);
		}
		if (!reader.AtEnd) { throw Invalid(); }
		return new MarkdownDocument(blocks);
	}

	public static bool IsSafeLink(string link)
	{
		if (link.Length > 4096 || link.IndexOf('\\') >= 0) { return false; }
		foreach (var character in link) { if (char.IsControl(character) || char.IsWhiteSpace(character)) { return false; } }
		if (!Uri.TryCreate(link, UriKind.Absolute, out var uri)) { return false; }
		if (uri.Scheme is "http" or "https") { return uri.Host.Length != 0 && uri.UserInfo.Length == 0 && !uri.Host.StartsWith('.'); }
		var destination = link[(link.IndexOf(':') + 1)..];
		return uri.Scheme == "mailto" && destination.Contains('@') && !destination.StartsWith('@') && !destination.EndsWith('@');
	}

	private static IOException Invalid() => new("Invalid mcw Markdown presentation response.");
	private ref struct Reader(ReadOnlySpan<byte> payload)
	{
		private ReadOnlySpan<byte> _remaining = payload;
		public readonly bool AtEnd => _remaining.IsEmpty;
		public byte Byte()
		{
			if (_remaining.IsEmpty) { throw Invalid(); }
			var value = _remaining[0]; _remaining = _remaining[1..]; return value;
		}
		public int Count(int maximum)
		{
			if (_remaining.Length < 4) { throw Invalid(); }
			var count = BinaryPrimitives.ReadUInt32LittleEndian(_remaining); _remaining = _remaining[4..];
			if (count > maximum) { throw Invalid(); }
			return (int)count;
		}
		public string String()
		{
			var length = Count(MaximumInputBytes);
			if (length > _remaining.Length) { throw Invalid(); }
			string text;
			try { text = Utf8.GetString(_remaining[..length]); } catch (DecoderFallbackException) { throw Invalid(); }
			_remaining = _remaining[length..];
			if (text.IndexOf('\0') >= 0) { throw Invalid(); }
			return text;
		}
	}
}
