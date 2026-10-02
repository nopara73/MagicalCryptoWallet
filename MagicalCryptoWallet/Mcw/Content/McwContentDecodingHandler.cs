using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Net.Http;
using System.Threading;
using System.Threading.Tasks;

namespace MagicalCryptoWallet.Mcw.Content;

/// <summary>The sole selected client identity; other transports keep their decoder.</summary>
public sealed class McwContentDecodingHandler(HttpMessageHandler innerHandler) : DelegatingHandler(innerHandler)
{
	public const string ClientName = "MempoolSpace-bitcoin-fee-rate-provider";

	protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
	{
		var services = McwApplicationServices.Current;
		using var linked = CancellationTokenSource.CreateLinkedTokenSource(cancellationToken, services.Stopped);
		linked.Token.ThrowIfCancellationRequested();
		if (request.Headers.AcceptEncoding.Count == 0) { request.Headers.AcceptEncoding.ParseAdd("gzip, deflate, br"); }
		var response = await base.SendAsync(request, linked.Token).ConfigureAwait(false);
		try
		{
			var original = response.Content;
			var encodedLength = original.Headers.ContentLength;
			if (encodedLength is < 0 or > McwContentDecoder.MaxBody)
			{ throw new IOException("The bounded encoded response exceeds its limit."); }
			var values = original.Headers.TryGetValues("Content-Encoding", out var fields) ? fields.ToArray() : [];
			if (values.Length > McwContentDecoder.MaxFields || values.Any(v => v.Length > McwContentDecoder.MaxField))
			{ throw new IOException("Invalid content encoding metadata."); }
			var input = await original.ReadAsStreamAsync(linked.Token).ConfigureAwait(false);
			using var buffer = new MemoryStream((int)(encodedLength ?? 8192));
			var chunk = new byte[8192];
			while (true)
			{
				var count = await input.ReadAsync(chunk, linked.Token).ConfigureAwait(false);
				if (count == 0) { break; }
				if (buffer.Length > McwContentDecoder.MaxBody - count)
				{ throw new IOException("The bounded encoded response exceeds its limit."); }
				buffer.Write(chunk, 0, count);
			}
			if (encodedLength.HasValue && encodedLength.Value != buffer.Length)
			{ throw new IOException("The encoded response length does not match its framing."); }
			var decoded = await McwContentDecoder.DecodeAsync(buffer.GetBuffer().AsMemory(0, (int)buffer.Length), values, services, linked.Token).ConfigureAwait(false);
			linked.Token.ThrowIfCancellationRequested();
			var content = new McwDecodedHttpContent(decoded, encodedLength);
			foreach (var header in original.Headers)
			{
				if (!header.Key.Equals("Content-Encoding", StringComparison.OrdinalIgnoreCase)
					&& !header.Key.Equals("Content-Length", StringComparison.OrdinalIgnoreCase))
				{ content.Headers.TryAddWithoutValidation(header.Key, header.Value); }
			}
			content.Headers.ContentLength = decoded.DecodedLength;
			response.Content = content;
			original.Dispose();
			return response;
		}
		catch { response.Dispose(); throw; }
	}
}

/// <summary>Wire-size proof stays available after Content-Length becomes decoded size.</summary>
public sealed class McwDecodedHttpContent : ByteArrayContent
{
	internal McwDecodedHttpContent(McwDecodedContent decoded, long? encodedContentLength) : base(decoded.Bytes)
	{ EncodedLength = decoded.EncodedLength; DecodedLength = decoded.DecodedLength; EncodedContentLength = encodedContentLength; Layers = decoded.Layers; }
	public int EncodedLength { get; }
	public int DecodedLength { get; }
	public long? EncodedContentLength { get; }
	public IReadOnlyList<McwContentLayerProof> Layers { get; }
}
