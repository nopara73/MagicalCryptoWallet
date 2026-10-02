using NBitcoin;
using System;
using System.IO;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Blocks;
using MagicalCryptoWallet.Wallets;
using Xunit;

namespace MagicalCryptoWallet.Tests.UnitTests.Wallet;

public class FileSystemBlockRepositoryTests
{
	[Fact]
	public async Task PrunesFilesOverConfiguredSizeAsync()
	{
		using var directory = new SyntheticDirectory();
		var workDir = directory.Path;

		try
		{
			Directory.CreateDirectory(workDir);
			var oldFilePath = Path.Combine(workDir, "old-block");
			await File.WriteAllBytesAsync(oldFilePath, new byte[1024 * 1024]);
			File.SetLastAccessTimeUtc(oldFilePath, DateTime.UtcNow - TimeSpan.FromDays(1));

			var block = Network.Main.Consensus.ConsensusFactory.CreateBlock();
			block.Header.Nonce = 1;
			var services = new HeaderServices(block.Header.ToBytes(), block.GetHash().ToBytes());
			var repository = new FileSystemBlockRepository(workDir, Network.Main, targetBlocksFolderSizeInMegabytes: 1, services);

			await repository.SaveAsync(block, CancellationToken.None);

			Assert.False(File.Exists(oldFilePath));
			Assert.True(File.Exists(Path.Combine(workDir, block.GetHash().ToString())));
		}
		finally
		{
			await IoHelpers.TryDeleteDirectoryAsync(workDir);
		}
	}

	[Fact]
	public async Task KeepsExistingFilenameAndBytesAsync()
	{
		using var directory = new SyntheticDirectory();
		var block = SyntheticBlock();
		var hash = block.GetHash();
		var services = new HeaderServices(block.Header.ToBytes(), hash.ToBytes());
		var repository = new FileSystemBlockRepository(directory.Path, Network.Main, 300, applicationServices: services);
		await repository.SaveAsync(block, CancellationToken.None);
		var path = Path.Combine(directory.Path, hash.ToString());
		Assert.Equal(block.ToBytes(), await File.ReadAllBytesAsync(path));
		var loaded = await repository.TryGetBlockAsync(hash, CancellationToken.None);
		Assert.NotNull(loaded);
		Assert.Equal(block.ToBytes(), loaded.ToBytes());
		Assert.Equal(2, services.Requests);
	}

	[Fact]
	public async Task SaveHashesAndWritesTheSameSnapshotAsync()
	{
		using var directory = new SyntheticDirectory();
		var block = SyntheticBlock();
		var bytes = block.ToBytes();
		var hash = block.GetHash();
		var services = new DelayedHeaderServices();
		var repository = new FileSystemBlockRepository(directory.Path, Network.Main, 300, services);
		var saving = repository.SaveAsync(block, CancellationToken.None);
		var requestedHeader = await services.Requested.Task;
		block.Header.Nonce++;
		services.Digest.SetResult(hash.ToBytes());
		await saving;
		Assert.Equal(bytes.AsSpan(0, 80).ToArray(), requestedHeader);
		Assert.Equal(bytes, await File.ReadAllBytesAsync(Path.Combine(directory.Path, hash.ToString())));
	}

	[Fact]
	public async Task WrongFilenameIsDeletedAndCanBeRedownloadedAsync()
	{
		using var directory = new SyntheticDirectory();
		var block = SyntheticBlock();
		var hash = block.GetHash();
		var wrongHash = new uint256(1);
		var wrongPath = Path.Combine(directory.Path, wrongHash.ToString());
		await File.WriteAllBytesAsync(wrongPath, block.ToBytes());
		var services = new HeaderServices(block.Header.ToBytes(), hash.ToBytes());
		var repository = new FileSystemBlockRepository(directory.Path, Network.Main, 300, applicationServices: services);
		Assert.Null(await repository.TryGetBlockAsync(wrongHash, CancellationToken.None));
		Assert.False(File.Exists(wrongPath));
		await repository.SaveAsync(block, CancellationToken.None);
		Assert.NotNull(await repository.TryGetBlockAsync(hash, CancellationToken.None));
	}

	[Fact]
	public async Task TruncatedHeaderIsDeletedWithoutHostRequestAsync()
	{
		using var directory = new SyntheticDirectory();
		var hash = new uint256(2);
		var path = Path.Combine(directory.Path, hash.ToString());
		await File.WriteAllBytesAsync(path, new byte[79]);
		var services = new HeaderServices(new byte[80], new byte[32]);
		var repository = new FileSystemBlockRepository(directory.Path, Network.Main, 300, applicationServices: services);
		Assert.Null(await repository.TryGetBlockAsync(hash, CancellationToken.None));
		Assert.False(File.Exists(path));
		Assert.Equal(0, services.Requests);
	}

	[Fact]
	public async Task CorruptBodyStillUsesRetainedBlockMappingAsync()
	{
		using var directory = new SyntheticDirectory();
		var block = SyntheticBlock();
		var hash = block.GetHash();
		var path = Path.Combine(directory.Path, hash.ToString());
		var bytes = new byte[81];
		block.Header.ToBytes().CopyTo(bytes, 0);
		bytes[80] = 1; // One transaction declared, with no transaction bytes.
		await File.WriteAllBytesAsync(path, bytes);
		var services = new HeaderServices(block.Header.ToBytes(), hash.ToBytes());
		var repository = new FileSystemBlockRepository(directory.Path, Network.Main, 300, applicationServices: services);
		Assert.Null(await repository.TryGetBlockAsync(hash, CancellationToken.None));
		Assert.False(File.Exists(path));
	}

	[Fact]
	public async Task HostFailurePreservesCacheAndAllowsRetryAsync()
	{
		using var directory = new SyntheticDirectory();
		var block = SyntheticBlock();
		var hash = block.GetHash();
		var path = Path.Combine(directory.Path, hash.ToString());
		var bytes = block.ToBytes();
		await File.WriteAllBytesAsync(path, bytes);
		var failed = new HeaderServices(block.Header.ToBytes(), hash.ToBytes(), new IOException("Synthetic host disconnection."));
		var repository = new FileSystemBlockRepository(directory.Path, Network.Main, 300, applicationServices: failed);
		await Assert.ThrowsAsync<McwBlockHeaderServiceException>(() => repository.TryGetBlockAsync(hash, CancellationToken.None));
		Assert.Equal(bytes, await File.ReadAllBytesAsync(path));
		var working = new HeaderServices(block.Header.ToBytes(), hash.ToBytes());
		var retry = new FileSystemBlockRepository(directory.Path, Network.Main, 300, applicationServices: working);
		Assert.NotNull(await retry.TryGetBlockAsync(hash, CancellationToken.None));
	}

	[Fact]
	public async Task InvalidHostDigestPreservesCacheAsync()
	{
		using var directory = new SyntheticDirectory();
		var block = SyntheticBlock();
		var hash = block.GetHash();
		var path = Path.Combine(directory.Path, hash.ToString());
		var bytes = block.ToBytes();
		await File.WriteAllBytesAsync(path, bytes);
		var services = new HeaderServices(block.Header.ToBytes(), new byte[31]);
		var repository = new FileSystemBlockRepository(directory.Path, Network.Main, 300, applicationServices: services);
		await Assert.ThrowsAsync<McwBlockHeaderServiceException>(() => repository.TryGetBlockAsync(hash, CancellationToken.None));
		Assert.Equal(bytes, await File.ReadAllBytesAsync(path));
	}

	[Fact]
	public async Task CancellationPreservesCacheAsync()
	{
		using var directory = new SyntheticDirectory();
		var block = SyntheticBlock();
		var hash = block.GetHash();
		var path = Path.Combine(directory.Path, hash.ToString());
		var bytes = block.ToBytes();
		await File.WriteAllBytesAsync(path, bytes);
		var services = new HeaderServices(block.Header.ToBytes(), hash.ToBytes());
		var repository = new FileSystemBlockRepository(directory.Path, Network.Main, 300, applicationServices: services);
		using var canceled = new CancellationTokenSource();
		canceled.Cancel();
		await Assert.ThrowsAnyAsync<OperationCanceledException>(() => repository.TryGetBlockAsync(hash, canceled.Token));
		Assert.Equal(bytes, await File.ReadAllBytesAsync(path));
		Assert.NotNull(await repository.TryGetBlockAsync(hash, CancellationToken.None));
	}

	private static Block SyntheticBlock()
	{
		var block = Network.Main.Consensus.ConsensusFactory.CreateBlock();
		block.Header.Version = 4;
		block.Header.Nonce = 0x80000001;
		return block;
	}

	// Fault-injection tests use a predetermined reference digest. The real-host
	// integration probe separately executes native hashing and these cache paths.
	private sealed class HeaderServices(byte[] expectedHeader, byte[] digest, Exception? failure = null) : IMcwApplicationServices
	{
		public int Requests { get; private set; }
		public CancellationToken Stopped => CancellationToken.None;
		public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
		{
			cancellationToken.ThrowIfCancellationRequested();
			Assert.Equal(McwBlockHeaderService.HashHeaderOperation, operation);
			Assert.Equal(expectedHeader, payload.ToArray());
			Requests++;
			return failure is null ? Task.FromResult((byte[])digest.Clone()) : Task.FromException<byte[]>(failure);
		}
	}

	private sealed class DelayedHeaderServices : IMcwApplicationServices
	{
		public TaskCompletionSource<byte[]> Requested { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
		public TaskCompletionSource<byte[]> Digest { get; } = new(TaskCreationOptions.RunContinuationsAsynchronously);
		public CancellationToken Stopped => CancellationToken.None;
		public Task<byte[]> RequestAsync(ushort operation, ReadOnlyMemory<byte> payload, CancellationToken cancellationToken = default)
		{
			Assert.Equal(McwBlockHeaderService.HashHeaderOperation, operation);
			Requested.SetResult(payload.ToArray());
			return Digest.Task.WaitAsync(cancellationToken);
		}
	}

	private sealed class SyntheticDirectory : IDisposable
	{
		private static readonly string Root = System.IO.Path.GetFullPath(System.IO.Path.Combine(System.IO.Path.GetTempPath(), "mcw-block-cache-tests"));
		public SyntheticDirectory()
		{
			Path = System.IO.Path.Combine(Root, Guid.NewGuid().ToString("N"));
			Directory.CreateDirectory(Path);
		}
		public string Path { get; }
		public void Dispose()
		{
			if (!System.IO.Path.GetFullPath(Path).StartsWith(Root + System.IO.Path.DirectorySeparatorChar, StringComparison.OrdinalIgnoreCase))
			{
				throw new InvalidOperationException("Synthetic cache path escaped the test directory.");
			}
			if (Directory.Exists(Path)) { Directory.Delete(Path, recursive: true); }
		}
	}
}
