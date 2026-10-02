using System;
using System.Collections.Generic;
using System.Text.Json;
using System.Text.Json.Nodes;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;
using MagicalCryptoWallet.Serialization;
using MagicalCryptoWallet.WabiSabi.Models;

namespace MagicalCryptoWallet.Mcw.Scripts;

/// <summary>
/// The WabiSabi HTTP client's existing message schema with native Script text
/// leaves. Coordinator and other shared serializer entry points remain separate.
/// </summary>
public static class ClientScriptTextJson
{
	public static async Task<JsonNode> EncodeRequestAsync<T>(T request, CancellationToken cancellationToken = default) where T : class
	{
		ArgumentNullException.ThrowIfNull(request);
		if (request is OutputRegistrationRequest output)
		{
			var text = await ScriptTextClient.RenderAsync(output.Script.ToBytes(), cancellationToken).ConfigureAwait(false);
			return Encode.ClientOutputRegistrationRequest(output, text);
		}
		return EncodeRequest(request, cancellationToken);
	}

	public static async Task<T> DecodeResponseAsync<T>(string json, CancellationToken cancellationToken = default)
	{
		ArgumentNullException.ThrowIfNull(json);
		if (typeof(T) != typeof(RoundStateResponse)) { return DecodeResponse<T>(json, cancellationToken); }

		// Resolve the native leaves asynchronously before the retained synchronous
		// schema decoder runs. Each occurrence receives its own native result;
		// nothing is cached across responses or replaced with a managed parse.
		using var document = JsonDocument.Parse(json);
		var scripts = new Dictionary<string, Queue<byte[]>>(StringComparer.Ordinal);
		foreach (var text in StatusScriptTexts(document.RootElement))
		{
			var bytes = await ScriptTextClient.ParseAsync(text, cancellationToken).ConfigureAwait(false);
			if (!scripts.TryGetValue(text, out var results)) { scripts.Add(text, results = new()); }
			results.Enqueue(bytes);
		}
		cancellationToken.ThrowIfCancellationRequested();
		var response = JsonDecoder.FromString(json, Decode.ClientRoundStateResponse(text =>
			new Script(scripts.TryGetValue(text, out var results) && results.TryDequeue(out var bytes)
				? bytes : throw new FormatException("Missing native Script text result."))))
			?? throw new FormatException("Invalid coordinator round-state response.");
		return (T)(object)response;
	}

	private static IEnumerable<string> StatusScriptTexts(JsonElement root)
	{
		foreach (var round in Property(root, "roundStates").EnumerateArray())
		{
			foreach (var entry in Property(Property(round, "coinjoinState"), "Events").EnumerateArray())
			{
				JsonElement txOut;
				switch (Property(entry, "Type").GetString())
				{
					case "InputAdded": txOut = Property(Property(entry, "Coin"), "TxOut"); break;
					case "OutputAdded": txOut = Property(entry, "Output"); break;
					default: continue; // The retained decoder validates every event kind.
				}
				yield return Property(txOut, "ScriptPubKey").GetString()
					?? throw new FormatException("Invalid coordinator Script text field.");
			}
		}
	}

	private static JsonElement Property(JsonElement element, string name)
	{
		if (element.TryGetProperty(name, out var value)) { return value; }
		foreach (var property in element.EnumerateObject())
		{
			if (string.Equals(property.Name, name, StringComparison.OrdinalIgnoreCase)) { return property.Value; }
		}
		throw new FormatException("Missing coordinator response field.");
	}

	public static JsonNode EncodeRequest<T>(T request, CancellationToken cancellationToken = default) where T : class
	{
		ArgumentNullException.ThrowIfNull(request);
		return request switch
		{
			OutputRegistrationRequest output => Encode.ClientOutputRegistrationRequest(output, cancellationToken),
			InputRegistrationRequest or InputsRemovalRequest or ConnectionConfirmationRequest
				or ReissueCredentialRequest or ReadyToSignRequestRequest or TransactionSignaturesRequest
				or RoundStateRequest => Encode.CoordinatorMessage(request),
			_ => throw new NotSupportedException($"{request.GetType().FullName} is not a WabiSabi client request.")
		};
	}

	public static T DecodeResponse<T>(string json, CancellationToken cancellationToken = default)
	{
		ArgumentNullException.ThrowIfNull(json);
		if (typeof(T) == typeof(RoundStateResponse))
		{
			return (T)(object)JsonDecoder.FromString(json, Decode.ClientRoundStateResponse(cancellationToken))!;
		}

		if (typeof(T) == typeof(InputRegistrationResponse)
			|| typeof(T) == typeof(ConnectionConfirmationResponse)
			|| typeof(T) == typeof(ReissueCredentialResponse))
		{
			return Decode.CoordinatorMessage<T>(json);
		}

		throw new NotSupportedException($"{typeof(T).FullName} is not a WabiSabi client response.");
	}
}
