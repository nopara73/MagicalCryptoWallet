using System.IO.Pipelines;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Mcw.Privacy;
using MagicalCryptoWallet.Tor.Control.Messages;

namespace MagicalCryptoWallet.Tor.Control;

/// <summary>Wallet reply parsing is owned by the mcw application service.</summary>
public class TorControlReplyReader
{
	public static Task<TorControlReply> ReadReplyAsync(PipeReader reader, CancellationToken cancellationToken)
		=> McwTorControlCodec.ReadReplyAsync(reader, cancellationToken);
}
