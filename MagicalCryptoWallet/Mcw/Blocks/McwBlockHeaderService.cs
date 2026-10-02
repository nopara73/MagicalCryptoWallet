using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using NBitcoin;

namespace MagicalCryptoWallet.Mcw.Blocks;

/// <summary>Exact serialized-header hashing through the one mcw application host.</summary>
public sealed class McwBlockHeaderService(IMcwApplicationServices? applicationServices = null)
{
	public const ushort HashHeaderOperation = 0x0E00;
	public const int HeaderLength = 80;

	public async Task<uint256> HashAsync(ReadOnlyMemory<byte> header, CancellationToken cancellationToken)
	{
		cancellationToken.ThrowIfCancellationRequested();
		if (header.Length != HeaderLength)
		{
			throw new InvalidDataException("A Bitcoin block header must contain exactly 80 bytes.");
		}

		try
		{
			var services = applicationServices ?? McwApplicationServices.Current;
			var digest = await services.RequestAsync(HashHeaderOperation, header, cancellationToken).ConfigureAwait(false);
			cancellationToken.ThrowIfCancellationRequested();
			if (digest.Length != 32)
			{
				throw new IOException("The mcw block header service returned an invalid digest length.");
			}

			// uint256 maps the raw little-endian digest to the retained managed data
			// model and existing display filename; it performs no header hashing.
			return new uint256(digest);
		}
		catch (OperationCanceledException)
		{
			throw;
		}
		catch (Exception error)
		{
			throw new McwBlockHeaderServiceException(error);
		}
	}
}

/// <summary>A host failure must not be treated as a corrupt cached block.</summary>
public sealed class McwBlockHeaderServiceException(Exception innerException)
	: IOException("The mcw block header service is unavailable.", innerException);
