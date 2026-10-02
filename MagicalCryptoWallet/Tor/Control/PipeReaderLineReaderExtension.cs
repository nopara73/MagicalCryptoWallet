using System.IO.Pipelines;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Mcw.Privacy;

namespace MagicalCryptoWallet.Tor.Control;

/// <summary>Wallet CRLF parsing is owned by the mcw application service.</summary>
public static class PipeReaderLineReaderExtension
{
	public static ValueTask<string> ReadLineAsync(this PipeReader reader, CancellationToken cancellationToken = default)
		=> McwTorControlCodec.ReadLineAsync(reader, cancellationToken);
}
