using System;
using System.Buffers.Binary;
using System.IO;
using System.Text;
using System.Threading;

namespace MagicalCryptoWallet.Mcw.Storage;

/// <summary>Serialized bytes only. The caller retains wallet/schema/encoding ownership.</summary>
public static class McwSafeFile
{
	private const ushort Begin = 0x1000, Append = 0x1001, Commit = 0x1002, Abort = 0x1003, Prepare = 0x1004;
	private const int ChunkSize = 256 * 1024, CharacterChunk = 8192;
	private static readonly Encoding PathEncoding = new UTF8Encoding(false, true);
	private static readonly bool DisableFileLocking = ReadFileLockingConfiguration();
	private static bool ReadFileLockingConfiguration()
	{
		string? environment = Environment.GetEnvironmentVariable("DOTNET_SYSTEM_IO_DISABLEFILELOCKING");
		if (environment == "1" || string.Equals(environment, "true", StringComparison.OrdinalIgnoreCase)) { return true; }
		if (environment == "0" || string.Equals(environment, "false", StringComparison.OrdinalIgnoreCase)) { return false; }
		return AppContext.TryGetSwitch("System.IO.DisableFileLocking", out bool configured) && configured;
	}

	public static void WriteAllBytes(string filePath, byte[] content)
	{
		using var write = new PendingWrite(filePath);
		ArgumentNullException.ThrowIfNull(content, "bytes");
		write.Open((ulong)content.LongLength, 0);
		write.Bytes(content);
		write.Complete(filePath);
	}

	public static void WriteAllText(string filePath, string? text, Encoding encoding)
	{
		using var write = new PendingWrite(filePath);
		ArgumentNullException.ThrowIfNull(encoding);
		text ??= string.Empty;
		byte[] preamble = encoding.GetPreamble();
		// File.WriteAllText validates a large encoding for preallocation before
		// opening the file. Short input is encoded only after .new is truncated.
		ulong total = text.Length < CharacterChunk ? ulong.MaxValue : checked((ulong)preamble.Length + (ulong)encoding.GetByteCount(text));
		write.Open(total, text.Length < CharacterChunk ? 0 : total);
		if (text.Length == 0)
		{
			write.Bytes(preamble);
		}
		else
		{
			byte[] buffer = new byte[checked(preamble.Length + encoding.GetMaxByteCount(Math.Min(text.Length, CharacterChunk)))];
			preamble.CopyTo(buffer, 0);
			int prefix = preamble.Length;
			Encoder encoder = encoding.GetEncoder();
			for (int offset = 0; offset < text.Length;)
			{
				int count = Math.Min(CharacterChunk, text.Length - offset);
				int encoded = encoder.GetBytes(text.AsSpan(offset, count), buffer.AsSpan(prefix), offset + count == text.Length);
				write.Bytes(buffer.AsSpan(0, prefix + encoded));
				prefix = 0;
				offset += count;
			}
		}
		write.Complete(filePath);
	}

	private sealed class PendingWrite : IDisposable
	{
		private readonly IMcwApplicationServices _services;
		private readonly byte[] _path;
		private ulong? _token;
		private ulong _written;
		private bool _completed;

		public PendingWrite(string filePath)
		{
			// Append before normalizing, like the retained helper. This preserves
			// relative paths, dot components and trailing directory separators.
			string temporary = Path.GetFullPath(filePath + ".new");
			_path = PathEncoding.GetBytes(temporary[..^4]);
			if (_path.Length == 0 || _path.Length > 128 * 1024) { throw new PathTooLongException(); }
			_services = McwApplicationServices.Current;
			byte[] prepare = new byte[6 + _path.Length];
			BinaryPrimitives.WriteUInt16LittleEndian(prepare, 1);
			BinaryPrimitives.WriteUInt32LittleEndian(prepare.AsSpan(2), (uint)_path.Length);
			_path.CopyTo(prepare, 6);
			Check(Request(Prepare, prepare), false);
		}

		public void Open(ulong total, ulong allocation)
		{
			byte[] begin = new byte[23 + _path.Length];
			BinaryPrimitives.WriteUInt16LittleEndian(begin, 1);
			BinaryPrimitives.WriteUInt64LittleEndian(begin.AsSpan(2), total);
			BinaryPrimitives.WriteUInt64LittleEndian(begin.AsSpan(10), allocation);
			begin[18] = DisableFileLocking ? (byte)1 : (byte)0;
			BinaryPrimitives.WriteUInt32LittleEndian(begin.AsSpan(19), (uint)_path.Length);
			_path.CopyTo(begin, 23);
			byte[] opened = Request(Begin, begin);
			Check(opened, true);
			_token = BinaryPrimitives.ReadUInt64LittleEndian(opened.AsSpan(3));
		}

		public void Bytes(ReadOnlySpan<byte> content)
		{
			while (!content.IsEmpty)
			{
				int length = Math.Min(ChunkSize, content.Length);
				byte[] append = Token(18 + length);
				BinaryPrimitives.WriteUInt64LittleEndian(append.AsSpan(10), _written);
				content[..length].CopyTo(append.AsSpan(18));
				Check(Request(Append, append), false);
				_written = checked(_written + (ulong)length);
				content = content[length..];
			}
		}

		public void Complete(string filePath)
		{
			// The original File.Move validates the destination after writing .new.
			ArgumentNullException.ThrowIfNull(filePath, "destFileName");
			ArgumentException.ThrowIfNullOrEmpty(filePath, "destFileName");
			_ = Path.GetFullPath(filePath);
			byte[] commit = Token(18);
			BinaryPrimitives.WriteUInt64LittleEndian(commit.AsSpan(10), _written);
			Check(Request(Commit, commit), false);
			_completed = true;
		}

		private byte[] Token(int size)
		{
			byte[] bytes = new byte[size];
			BinaryPrimitives.WriteUInt16LittleEndian(bytes, 1);
			BinaryPrimitives.WriteUInt64LittleEndian(bytes.AsSpan(2), _token ?? throw new InvalidOperationException("Safe file stream is not open."));
			return bytes;
		}

		private byte[] Request(ushort operation, byte[] payload)
			=> _services.RequestAsync(operation, payload, CancellationToken.None).GetAwaiter().GetResult();

		public void Dispose()
		{
			if (_token.HasValue && !_completed && !_services.Stopped.IsCancellationRequested)
			{
				// Close .new without promotion. Preserve the primary exception and
				// never retry an uncertain commit or fall back to managed file I/O.
				try { Request(Abort, Token(10)); }
				catch (Exception) { }
			}
		}
	}

	private static void Check(byte[] reply, bool token)
	{
		if (reply.Length < 3 || BinaryPrimitives.ReadUInt16LittleEndian(reply) != 1)
		{ throw new IOException("Invalid mcw safe-file response."); }
		if (reply[2] == 0 && reply.Length == (token ? 11 : 3)) { return; }
		if (reply[2] != 1 || reply.Length != 9) { throw new IOException("Invalid mcw safe-file response."); }
		byte stage = reply[3], kind = reply[4];
		int nativeError = BinaryPrimitives.ReadInt32LittleEndian(reply.AsSpan(5));
		string message = $"Safe file write failed at stage {stage} (native error {nativeError}).";
		if (stage == 10 && !OperatingSystem.IsWindows()) { throw new IOException(message); }
		if ((OperatingSystem.IsWindows() && nativeError == 206) || (OperatingSystem.IsLinux() && nativeError == 36) || (OperatingSystem.IsMacOS() && nativeError == 63))
		{ throw new PathTooLongException(message); }
		int hresult = OperatingSystem.IsWindows() ? unchecked((int)0x80070000) | nativeError : nativeError;
		if (OperatingSystem.IsWindows() && nativeError != 0)
		{
			throw nativeError switch
			{
				2 => new FileNotFoundException(message),
				3 => new DirectoryNotFoundException(message),
				5 => new UnauthorizedAccessException(message),
				995 => new OperationCanceledException(message),
				_ => new IOException(message, hresult)
			};
		}
		throw kind switch
		{
			1 when stage is 6 or 7 => new FileNotFoundException(message),
			1 or 6 => new DirectoryNotFoundException(message),
			2 or 7 => new UnauthorizedAccessException(message),
			_ when nativeError != 0 => new IOException(message, hresult),
			_ => new IOException(message)
		};
	}
}
