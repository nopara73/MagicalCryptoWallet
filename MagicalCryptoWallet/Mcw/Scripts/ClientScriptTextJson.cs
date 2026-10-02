using System;
using System.Text.Json.Nodes;
using System.Threading;
using MagicalCryptoWallet.Serialization;
using MagicalCryptoWallet.WabiSabi.Models;

namespace MagicalCryptoWallet.Mcw.Scripts;

/// <summary>
/// The WabiSabi HTTP client's existing message schema with native Script text
/// leaves. Coordinator and other shared serializer entry points remain separate.
/// </summary>
public static class ClientScriptTextJson
{
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
