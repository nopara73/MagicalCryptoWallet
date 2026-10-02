using NBitcoin;
using Nito.AsyncEx;
using System.IO;
using System.Linq;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Extensions;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Blocks;

namespace MagicalCryptoWallet.Wallets;

public class FileSystemBlockRepository
{
	private const int MegaByte = 1024 * 1024;

	public FileSystemBlockRepository(string blocksFolderPath, Network network, int targetBlocksFolderSizeInMegabytes = 300)
		: this(blocksFolderPath, network, targetBlocksFolderSizeInMegabytes, applicationServices: null)
	{
	}

	public FileSystemBlockRepository(string blocksFolderPath, Network network, int targetBlocksFolderSizeInMegabytes, IMcwApplicationServices? applicationServices)
	{
		BlocksFolderPath = blocksFolderPath;
		_network = network;
		_targetBlocksFolderSizeBytes = targetBlocksFolderSizeInMegabytes * MegaByte;
		_headerService = new McwBlockHeaderService(applicationServices);
		RemoveBlockFolderForRegTest();
	}

	public string BlocksFolderPath { get; }
	private readonly Network _network;
	private readonly long _targetBlocksFolderSizeBytes;
	private readonly AsyncLock _blockFolderLock = new();
	private readonly McwBlockHeaderService _headerService;

	private void Prune()
	{
		try
		{
			var filesToDelete = Directory
				.EnumerateFiles(BlocksFolderPath)
				.Select(x => new FileInfo(x))
				.OrderByDescending(x => x.LastAccessTimeUtc)
				.Scan((fileInfo:(FileInfo)null!, accumSize: 0L), (acc, fileInfo) => (fileInfo, acc.accumSize + fileInfo.Length))
				.SkipWhile(x => x.accumSize < _targetBlocksFolderSizeBytes)
				.Select(x => x.fileInfo)
				.ToArray();

			foreach (var blockFile in filesToDelete)
			{
				try
				{
					blockFile.Delete();
				}
				catch (Exception ex)
				{
					Logger.LogWarning(ex);
				}
			}
		}
		catch (Exception ex)
		{
			Logger.LogWarning(ex);
		}
	}

	public async Task<Block?> TryGetBlockAsync(uint256 hash, CancellationToken cancellationToken)
	{
		EnsureBlockFolderExists();
		using (await _blockFolderLock.LockAsync(cancellationToken).ConfigureAwait(false))
		{
			var filePath = Path.Combine(BlocksFolderPath, hash.ToString());
			if (!File.Exists(filePath))
			{
				return null;
			}

			try
			{
				var blockBytes = await File.ReadAllBytesAsync(filePath, cancellationToken).ConfigureAwait(false);
				if (blockBytes.Length < McwBlockHeaderService.HeaderLength)
				{
					throw new InvalidDataException("Cached block header is truncated.");
				}
				var cachedHash = await _headerService.HashAsync(blockBytes.AsMemory(0, McwBlockHeaderService.HeaderLength), cancellationToken).ConfigureAwait(false);
				if (cachedHash != hash)
				{
					throw new InvalidDataException("Cached block header does not match its filename.");
				}
				var block = Block.Load(blockBytes, _network);

				UpdateLastAccessTime(filePath);
				return block;
			}
			catch(Exception e) when (e is not McwBlockHeaderServiceException and not OperationCanceledException)
			{
				Logger.LogDebug(e);
				Logger.LogInfo($"Block {hash} file corrupted, deleting file and block will be re-downloaded. Deleting...");
				File.Delete(filePath);
			}
		}
		return null;
	}

	private static void UpdateLastAccessTime(string filePath)
	{
		try
		{
			var fileInfo = new FileInfo(filePath);
			fileInfo.LastAccessTimeUtc = DateTime.UtcNow;
		}
		catch (Exception)
		{
			// ignored
		}
	}

	public async Task SaveAsync(Block block, CancellationToken cancellationToken)
	{
		EnsureBlockFolderExists();
		// Hash and save the same snapshot across the asynchronous host request.
		var blockBytes = block.ToBytes();
		var hash = await _headerService.HashAsync(blockBytes.AsMemory(0, McwBlockHeaderService.HeaderLength), cancellationToken).ConfigureAwait(false);
		var path = Path.Combine(BlocksFolderPath, hash.ToString());
		if (!File.Exists(path))
		{
			using (await _blockFolderLock.LockAsync(cancellationToken).ConfigureAwait(false))
			{
				if (!File.Exists(path))
				{
					await File.WriteAllBytesAsync(path, blockBytes, CancellationToken.None).ConfigureAwait(false);
					Prune();
				}
			}
		}
	}

	private void EnsureBlockFolderExists()
	{
		IoHelpers.EnsureDirectoryExists(BlocksFolderPath);
	}

	private void RemoveBlockFolderForRegTest()
	{
		try
		{
			if (Directory.Exists(BlocksFolderPath) && _network == Network.RegTest)
			{
				Directory.Delete(BlocksFolderPath, true);
			}
		}
		catch (Exception ex)
		{
			Logger.LogDebug(ex);
		}
	}
}
