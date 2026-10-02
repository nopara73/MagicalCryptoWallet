using System;
using System.Collections.Generic;
using System.IO;
using System.Threading;
using NBitcoin;
using MagicalCryptoWallet.Blockchain.Keys;
using MagicalCryptoWallet.Blockchain.Transactions;
using MagicalCryptoWallet.Logging;

namespace MagicalCryptoWallet.Mcw.Psbt;

/// <summary>Wallet metadata adapter. KeyManager and the transaction store remain
/// state owners; mcw owns packet inspection and metadata edits. The returned
/// NBitcoin container is temporary interoperability with the retained signer.</summary>
public static class McwPsbtMetadata
{
	public const ushort EnrichOperation = 0x0600;
	public const ushort InspectOperation = 0x0601;
	private const ushort Version = 1;
	private const int MaxPacket = 16 * 1024 * 1024;
	private const int MaxRequest = 32 * 1024 * 1024;
	private const int Chunk = 64 * 1024;
	private const ushort BeginOperation = 0x0602;
	private const ushort AppendOperation = 0x0603;
	private const ushort CommitOperation = 0x0604;
	private const ushort ReadOperation = 0x0605;
	private const ushort AbortOperation = 0x0606;
	private static readonly SemaphoreSlim TransferLock = new(1, 1);
	private const int MaxValue = 4 * 1024 * 1024;
	private const int MaxMaps = 20000;
	private sealed record Input(byte[] Txid, uint Vout, byte[]? WitnessScript);
	private sealed record Inspection(Input[] Inputs, byte[][] Outputs);
	private sealed record Origin(byte[] PublicKey, byte[] Fingerprint, uint[] Path, byte[] Script);
	private sealed record Parent(byte[] Txid, byte[] Transaction);

	public static PSBT Enrich(PSBT packet, KeyManager keyManager, ITransactionStore transactionStore)
	{
		ArgumentNullException.ThrowIfNull(packet);
		ArgumentNullException.ThrowIfNull(keyManager);
		ArgumentNullException.ThrowIfNull(transactionStore);
		// The wallet builder uses standard network extensions. Arbitrary custom
		// managed script plugins have no native representation in this small leaf.
		if (packet.Settings.CustomBuilderExtensions is not null)
		{
			throw new InvalidOperationException("Custom builder extensions are unsupported by wallet PSBT metadata enrichment.");
		}
		var service = McwApplicationServices.Current;
		var bytes = packet.ToBytes();
		var inspection = Inspect(service, bytes);
		var origins = new List<Origin>();
		var knownOrigins = new HashSet<string>(StringComparer.Ordinal);
		if (keyManager.MasterFingerprint is { } fingerprint)
		{
			foreach (var input in inspection.Inputs)
			{
				if (input.WitnessScript is { } script) { AddOrigin(script); }
			}
			foreach (var script in inspection.Outputs) { AddOrigin(script); }

			void AddOrigin(byte[] scriptBytes)
			{
				var script = new Script(scriptBytes);
				if (keyManager.TryGetKeyForScriptPubKey(script, out var key))
				{
					var publicKey = key.PubKey.ToBytes();
					if (knownOrigins.Add(Convert.ToHexString(publicKey) + Convert.ToHexString(scriptBytes)))
					{
						origins.Add(new Origin(publicKey, fingerprint.ToBytes(), key.FullKeyPath.Indexes, scriptBytes));
					}
				}
			}
		}
		var parents = new List<Parent>();
		var knownParents = new HashSet<uint256>();
		foreach (var input in inspection.Inputs)
		{
			var txid = new uint256(input.Txid);
			if (transactionStore.TryGetTransaction(txid, out var transaction))
			{
				if (knownParents.Add(txid)) { parents.Add(new Parent(input.Txid, transaction.Transaction.ToBytes())); }
			}
			else
			{
				Logger.LogDebug($"Transaction id: {txid} is missing from the {nameof(transactionStore)}. Ignoring...");
			}
		}
		using var request = new MemoryStream();
		using var writer = new BinaryWriter(request);
		writer.Write(Version);
		WriteBlob(writer, bytes, MaxPacket);
		writer.Write(packet.Settings.IsSmart);
		writer.Write(origins.Count);
		foreach (var origin in origins)
		{
			if (origin.PublicKey.Length != 33 || origin.Fingerprint.Length != 4) { throw InvalidPayload(); }
			writer.Write(origin.PublicKey);
			writer.Write(origin.Fingerprint);
			writer.Write(origin.Path.Length);
			foreach (var index in origin.Path) { writer.Write(index); }
			WriteBlob(writer, origin.Script, MaxValue);
		}
		writer.Write(parents.Count);
		foreach (var parent in parents)
		{
			writer.Write(parent.Txid);
			WriteBlob(writer, parent.Transaction, MaxValue);
		}
		using var reader = Reader(Call(service, EnrichOperation, request));
		var enrichedBytes = ReadBlob(reader, MaxPacket);
		Finish(reader);
		var enriched = PSBT.Load(enrichedBytes, packet.Network);
		enriched.Settings = packet.Settings.Clone();
		return enriched;
	}

	private static Inspection Inspect(IMcwApplicationServices service, byte[] packet)
	{
		using var request = new MemoryStream();
		using var writer = new BinaryWriter(request);
		writer.Write(Version);
		WriteBlob(writer, packet, MaxPacket);
		using var reader = Reader(Call(service, InspectOperation, request));
		var inputs = new Input[ReadCount(reader, MaxMaps, 37)];
		for (int index = 0; index < inputs.Length; index++)
		{
			var txid = ReadBytes(reader, 32);
			var vout = reader.ReadUInt32();
			var presence = reader.ReadByte();
			if (presence > 1) { throw InvalidPayload(); }
			inputs[index] = new Input(txid, vout, presence == 1 ? ReadBlob(reader, MaxValue) : null);
		}
		var outputs = new byte[ReadCount(reader, MaxMaps - inputs.Length, 4)][];
		for (int index = 0; index < outputs.Length; index++) { outputs[index] = ReadBlob(reader, MaxValue); }
		Finish(reader);
		return new Inspection(inputs, outputs);
	}
	private static byte[] Call(IMcwApplicationServices service, ushort operation, MemoryStream request)
	{
		if (request.Length > MaxRequest) { throw new IOException("PSBT metadata request exceeds the application limit."); }
		// Serialize only this leaf's transfers; wallet state stays outside the lock.
		TransferLock.Wait(service.Stopped);
		ulong session = 0;
		try
		{
			var payload = request.ToArray();
			using (var begin = Reader(Send(BeginOperation, writer => { writer.Write(operation); writer.Write(payload.Length); })))
			{
				session = begin.ReadUInt64();
				if (session == 0) { throw InvalidPayload(); }
				Finish(begin);
			}
			for (int offset = 0; offset < payload.Length;)
			{
				int length = Math.Min(Chunk, payload.Length - offset);
				using var append = Reader(Send(AppendOperation, writer =>
				{
					writer.Write(session); writer.Write(offset); writer.Write(length); writer.Write(payload, offset, length);
				}));
				if (append.ReadUInt64() != session || append.ReadUInt32() != offset + length) { throw InvalidPayload(); }
				Finish(append);
				offset += length;
			}
			int responseLength;
			using (var commit = Reader(Send(CommitOperation, writer => writer.Write(session))))
			{
				if (commit.ReadUInt64() != session) { throw InvalidPayload(); }
				var length = commit.ReadUInt32();
				if (length < 2 || length > MaxRequest) { throw InvalidPayload(); }
				responseLength = (int)length;
				Finish(commit);
			}
			var response = new byte[responseLength];
			for (int offset = 0; offset < responseLength;)
			{
				int length = Math.Min(Chunk, responseLength - offset);
				ReadChunk(session, offset, length).CopyTo(response, offset);
				offset += length;
			}
			// The native final read releases the private result automatically.
			session = 0;
			return response;
		}
		finally
		{
			if (session != 0 && !service.Stopped.IsCancellationRequested)
			{
				try { using var aborted = Reader(Send(AbortOperation, writer => writer.Write(session))); }
				catch (IOException) { }
				catch (OperationCanceledException) { }
			}
			TransferLock.Release();
		}

		byte[] ReadChunk(ulong id, int offset, int length)
		{
			using var reader = Reader(Send(ReadOperation, writer => { writer.Write(id); writer.Write(offset); writer.Write(length); }));
			if (reader.ReadUInt64() != id || reader.ReadUInt32() != offset) { throw InvalidPayload(); }
			var chunk = ReadBlob(reader, Chunk);
			if (chunk.Length != length) { throw InvalidPayload(); }
			Finish(reader);
			return chunk;
		}

		byte[] Send(ushort transferOperation, Action<BinaryWriter> write)
		{
			using var framePayload = new MemoryStream();
			using var writer = new BinaryWriter(framePayload);
			writer.Write(Version);
			write(writer);
			if (framePayload.Length > Chunk + 18) { throw InvalidPayload(); }
			var result = service.RequestAsync(transferOperation, framePayload.ToArray(), service.Stopped).GetAwaiter().GetResult();
			if (result.Length > Chunk + 18) { throw InvalidPayload(); }
			return result;
		}
	}
	private static BinaryReader Reader(byte[] bytes)
	{
		var reader = new BinaryReader(new MemoryStream(bytes, writable: false));
		if (bytes.Length < 2 || reader.ReadUInt16() != Version)
		{
			reader.Dispose();
			throw InvalidPayload();
		}
		return reader;
	}
	private static int ReadCount(BinaryReader reader, int maximum, int minimumBytes)
	{
		var count = reader.ReadUInt32();
		if (count > maximum || count > (reader.BaseStream.Length - reader.BaseStream.Position) / minimumBytes) { throw InvalidPayload(); }
		return (int)count;
	}
	private static byte[] ReadBlob(BinaryReader reader, int maximum) => ReadBytes(reader, ReadCount(reader, maximum, 1));
	private static byte[] ReadBytes(BinaryReader reader, int count)
	{
		var result = reader.ReadBytes(count);
		if (result.Length != count) { throw InvalidPayload(); }
		return result;
	}
	private static void WriteBlob(BinaryWriter writer, byte[] bytes, int maximum)
	{
		if (bytes.Length > maximum || writer.BaseStream.Length + bytes.Length + 4 > MaxRequest)
		{
			throw new IOException("PSBT metadata request exceeds the application limit.");
		}
		writer.Write(bytes.Length);
		writer.Write(bytes);
	}
	private static void Finish(BinaryReader reader)
	{
		if (reader.BaseStream.Position != reader.BaseStream.Length) { throw InvalidPayload(); }
	}
	private static IOException InvalidPayload() => new("Invalid mcw PSBT metadata response.");
}
