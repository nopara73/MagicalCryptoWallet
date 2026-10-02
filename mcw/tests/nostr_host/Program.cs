using System;
using System.Collections.Generic;
using System.IO;
using System.Linq;
using System.Text.Json;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Client.Application;
using MagicalCryptoWallet.Helpers;
using MagicalCryptoWallet.Logging;
using MagicalCryptoWallet.Mcw;
using MagicalCryptoWallet.Mcw.Nostr;
using MagicalCryptoWallet.WebClients;
using NBitcoin.Secp256k1;
using NNostr.Client;
using NNostr.Client.Protocols;

// Test-only managed child of the actual mcw host. No wallet, relay or HTTP access.
internal static class Program
{
	private const string Secret = "0000000000000000000000000000000000000000000000000000000000000001";
	private const string OtherSecret = "0000000000000000000000000000000000000000000000000000000000000002";
	private static int _digestComparisons;
	private static int _signatureChecks;
	private static int _callerChecks;
	private static int _failureChecks;

	private static async Task<int> Main(string[] args)
	{
		if (Environment.GetEnvironmentVariable("MCW_HOSTED") != "1")
		{
			throw new InvalidOperationException("Run this synthetic test child through mcw gui.");
		}
		if (args.Length != 2 || args[0] != "--evidence")
		{
			throw new ArgumentException("Expected --evidence and a test output path.");
		}
		Logger.Configure(logModes: [LogMode.Console]);
		using var host = ManagedApplicationHost.Connect();
		await ComparePinnedNnostrAsync();
		await VerifySignedEventsAsync();
		await ExerciseRetainedCallerAsync();
		await ExerciseHostErrorsAsync();
		host.Dispose();
		await RejectUnavailableHostAsync();
		var record = new
		{
			status = "passed",
			actual_native_host = true,
			actual_managed_host_adapter = true,
			actual_retained_update_caller = true,
			synthetic_only = true,
			digest_comparisons = _digestComparisons,
			signature_checks = _signatureChecks,
			caller_checks = _callerChecks,
			failure_checks = _failureChecks,
			retained = "NNostr NIP-19/relay/subscriptions/publishing; NBitcoin.Secp256k1 BIP340; .NET WebSocket/TLS",
			completed_utc = DateTimeOffset.UtcNow
		};
		await File.WriteAllTextAsync(args[1], JsonSerializer.Serialize(record, new JsonSerializerOptions { WriteIndented = true }));
		Console.Error.WriteLine(JsonSerializer.Serialize(record));
		return 0;
	}

	private static async Task ComparePinnedNnostrAsync()
	{
		using var key = NostrExtensions.ParseKey(Secret);
		var publicKey = key.CreateXOnlyPubKey().ToHex();
		var contents = new[]
		{
			string.Empty,
			"quote\" slash/ backslash\\ <tag> apostrophe'",
			new string(Enumerable.Range(0, 32).Select(i => (char)i).ToArray()),
			"\u00e9 e\u0301 \U0001f9d9 \u2028 \u2029 \uffff \U0010ffff",
			"\ud800", "\udfff", "\ud800x\udfff", "\ud800\ud800", "\udfff\udfff",
			"\ufeff \u007f \u0080 \u00a0 /",
			new string('\0', 100_000)
		};
		foreach (var content in contents)
		{
			foreach (var time in new DateTimeOffset?[]
			{
				null, DateTimeOffset.UnixEpoch, DateTimeOffset.FromUnixTimeSeconds(-1),
				new(2026, 10, 2, 0, 0, 0, TimeSpan.FromHours(8)), DateTimeOffset.MaxValue
			})
			{
				var note = new NostrEvent
				{
					PublicKey = publicKey, CreatedAt = time, Kind = 1, Content = content,
					Tags =
					[
						new() { TagIdentifier = "p", Data = [content, "same"] },
						new() { TagIdentifier = "p", Data = ["same", content] },
						new() { TagIdentifier = string.Empty, Data = [string.Empty] },
						new() { TagIdentifier = null!, Data = [null!, content] },
						new() { TagIdentifier = null!, Data = [] }
					]
				};
				await CompareAsync(note);
			}
		}
		foreach (var kind in new[] { int.MinValue, -1, 0, 1, 65535, int.MaxValue })
		{
			await CompareAsync(new NostrEvent { PublicKey = publicKey, Kind = kind, Content = null, Tags = [] });
		}
		// One independent NIP-01 literal, with its Python hashlib digest frozen.
		var empty = new NostrEvent { PublicKey = publicKey, CreatedAt = null, Kind = 1, Content = null, Tags = [] };
		Equal("1d60156c7d5c3d752ed401ba085300ea90869712b4acc88edff9601de4c0b15c",
			Convert.ToHexStringLower(await McwNostrEventId.ComputeDigestAsync(empty)), "independent empty event vector");
		_digestComparisons++;
	}

	private static async Task CompareAsync(NostrEvent note)
	{
		// Legacy code is an independent test oracle, never a production fallback.
		var expected = note.ComputeId();
		var actual = Convert.ToHexStringLower(await McwNostrEventId.ComputeDigestAsync(note));
		Equal(expected, actual, "pinned NNostr 0.0.55 event digest");
		_digestComparisons++;
	}

	private static async Task VerifySignedEventsAsync()
	{
		foreach (var content in new[] { "synthetic", "é e\u0301 🧙\u2028\u2029\0\n", "\ud800x\udfff" })
		{
			var note = await ReleaseAsync(content: content);
			Equal(true, note.Verify(), "legacy synthetic signature");
			Equal(true, McwNostrEventId.IsAuthentic(note), "Rust digest with managed BIP340");
			_signatureChecks++;
			note.Content += "tampered";
			Equal(false, McwNostrEventId.IsAuthentic(note), "tampered content");
			_signatureChecks++;
		}
		foreach (var mutation in new[] { "signature-bit", "signature-zero", "signature-overflow", "id-uppercase", "wrong-key", "curve-invalid" })
		{
			var note = await ReleaseAsync();
			switch (mutation)
			{
				case "signature-bit":
					var signature = Convert.FromHexString(note.Signature);
					signature[^1] ^= 1;
					note.Signature = Convert.ToHexStringLower(signature);
					break;
				case "signature-zero": note.Signature = new string('0', 128); break;
				case "signature-overflow": note.Signature = new string('f', 128); break;
				case "id-uppercase": note.Id = note.Id.ToUpperInvariant(); break;
				case "wrong-key":
					using (var other = NostrExtensions.ParseKey(OtherSecret)) note.PublicKey = other.CreateXOnlyPubKey().ToHex();
					note.Id = note.ComputeId();
					break;
				case "curve-invalid": note.PublicKey = new string('f', 64); note.Id = note.ComputeId(); break;
			}
			if (mutation == "curve-invalid")
			{
				try { note.Verify(); throw new Exception("Legacy verifier accepted an invalid curve point."); }
				catch (FormatException) { }
				try { McwNostrEventId.IsAuthentic(note); throw new Exception("Invalid curve point was accepted."); }
				catch (FormatException) { }
			}
			else Equal(false, McwNostrEventId.IsAuthentic(note), mutation);
			_signatureChecks++;
		}
	}

	private static async Task ExerciseRetainedCallerAsync()
	{
		using var key = NostrExtensions.ParseKey(Secret);
		var npub = key.CreateXOnlyPubKey().ToNIP19();
		foreach (var scenario in new[]
		{
			"valid", "unicode", "duplicate-event", "wrong-author", "forged-author", "tampered-content",
			"tampered-signature", "uppercase-id", "missing-manifest", "wrong-destination", "duplicate-version",
			"wrong-kind", "wrong-subscription", "empty-version", "invalid-signature-hex", "old-timestamp"
		})
		{
			using var transport = new SyntheticRelay();
			using var client = new MagicalCryptoWalletNostrClient(transport, npub);
			await client.ConnectAndSubscribeAsync(CancellationToken.None);
			Equal(1, transport.Filters![0].Kinds![0], "retained kind filter");
			Equal(key.CreateXOnlyPubKey().ToHex(), transport.Filters[0].Authors![0], "retained author filter");
			Equal(1, transport.Filters[0].Limit, "retained subscription limit");
			var note = await ReleaseAsync(
				secret: scenario is "wrong-author" or "forged-author" ? OtherSecret : Secret,
				content: scenario == "unicode" ? "é e\u0301 🧙\u2028\u2029\0\n" : "synthetic release",
				beforeSigning: n =>
				{
					switch (scenario)
					{
						case "missing-manifest": n.Tags.RemoveAll(t => t.TagIdentifier == "SHA256SUMS"); break;
						case "wrong-destination": n.Tags[1].Data = ["https://example.invalid/SHA256SUMS"]; break;
						case "duplicate-version": n.Tags.Add(new() { TagIdentifier = "version", Data = ["99.0.0"] }); break;
						case "empty-version": n.Tags[0].Data = []; break;
						case "wrong-kind": n.Kind = 2; break;
						case "old-timestamp": n.CreatedAt = DateTimeOffset.FromUnixTimeSeconds(-1); break;
					}
				});
			if (scenario == "forged-author") note.PublicKey = key.CreateXOnlyPubKey().ToHex();
			if (scenario == "tampered-content") note.Content += "tampered";
			if (scenario == "tampered-signature") note.Signature = new string('0', 128);
			if (scenario == "uppercase-id") note.Id = note.Id.ToUpperInvariant();
			if (scenario == "invalid-signature-hex") note.Signature = "zz";
			transport.Emit([note], wrongSubscription: scenario == "wrong-subscription");
			if (scenario == "duplicate-event") transport.Emit([note]);
			transport.Eose();
			var releases = await ReadAsync(client);
			var expected = scenario is "valid" or "unicode" or "duplicate-event" or "old-timestamp" ? 1 : 0;
			Equal(expected, releases.Count, scenario);
			if (expected == 1) Equal(new Version(2, 5, 0), releases[0].Version, scenario + " version");
			_callerChecks++;
		}
	}

	private static async Task ExerciseHostErrorsAsync()
	{
		try
		{
			await McwApplicationServices.Current.RequestAsync(McwNostrEventId.Operation, new byte[] { 2 });
			throw new Exception("Malformed host request was accepted.");
		}
		catch (IOException) { _failureChecks++; }
		using (var cancel = new CancellationTokenSource())
		{
			cancel.Cancel();
			try { await McwNostrEventId.ComputeDigestAsync(await ReleaseAsync(), cancel.Token); throw new Exception("Cancelled request succeeded."); }
			catch (OperationCanceledException) { _failureChecks++; }
		}
		var oversized = await ReleaseAsync();
		oversized.Content = new string('a', 1_048_560);
		try { await McwNostrEventId.ComputeDigestAsync(oversized); throw new Exception("Oversized request succeeded."); }
		catch (ArgumentException) { _failureChecks++; }
		Equal(true, McwNostrEventId.IsAuthentic(await ReleaseAsync()), "host remains usable after rejected requests");
		_failureChecks++;
	}

	private static async Task RejectUnavailableHostAsync()
	{
		var note = await ReleaseAsync();
		Equal(true, note.Verify(), "legacy valid event used for no-fallback check");
		try { McwNostrEventId.IsAuthentic(note); throw new Exception("Unavailable host used a legacy fallback."); }
		catch (InvalidOperationException) { _failureChecks++; }
		using var key = NostrExtensions.ParseKey(Secret);
		using var transport = new SyntheticRelay();
		using var client = new MagicalCryptoWalletNostrClient(transport, key.CreateXOnlyPubKey().ToNIP19());
		await client.ConnectAndSubscribeAsync(CancellationToken.None);
		transport.Emit([note]);
		transport.Eose();
		Equal(0, (await ReadAsync(client)).Count, "retained caller fails closed without mcw");
		_failureChecks++;
	}

	private static async Task<NostrEvent> ReleaseAsync(string secret = Secret, string content = "synthetic release", Action<NostrEvent>? beforeSigning = null)
	{
		var version = new Version(2, 5, 0);
		var note = new NostrEvent
		{
			Kind = 1, CreatedAt = DateTimeOffset.FromUnixTimeSeconds(1_700_000_000), Content = content,
			Tags = [new() { TagIdentifier = "version", Data = [version.ToString()] }]
		};
		foreach (var name in new[] { "SHA256SUMS", "SHA256SUMS.asc", "SHA256SUMS.magicalcryptowalletsig", "MagicalCryptoWallet-2.5.0.msi" })
		{
			note.Tags.Add(new() { TagIdentifier = name, Data = [$"{Constants.RepositoryUrl}/releases/download/v{version}/{name}"] });
		}
		beforeSigning?.Invoke(note);
		using var key = NostrExtensions.ParseKey(secret);
		return await note.ComputeIdAndSignAsync(key);
	}

	private static async Task<List<ReleaseInfo>> ReadAsync(MagicalCryptoWalletNostrClient client)
	{
		var releases = new List<ReleaseInfo>();
		await foreach (var release in client.EventsReader.ReadAllAsync()) releases.Add(release);
		return releases;
	}

	private static void Equal<T>(T expected, T actual, string context)
	{
		if (!EqualityComparer<T>.Default.Equals(expected, actual)) throw new Exception("Synthetic check failed: " + context);
	}
}

internal sealed class SyntheticRelay : INostrClient
{
	private string? _subscription;
	public NostrSubscriptionFilter[]? Filters { get; private set; }
	public Task CreateSubscription(string subscriptionId, NostrSubscriptionFilter[] filters, CancellationToken token)
	{
		_subscription = subscriptionId;
		Filters = filters;
		return Task.CompletedTask;
	}
	public void Emit(NostrEvent[] events, bool wrongSubscription = false) => EventsReceived?.Invoke(this, (wrongSubscription ? "wrong-subscription" : _subscription!, events));
	public void Eose() => EoseReceived?.Invoke(this, _subscription!);
	public Task Connect(CancellationToken token) => Task.CompletedTask;
	public Task ConnectAndWaitUntilConnected(CancellationToken connectionCancellationToken, CancellationToken lifetimeCancellationToken) => Task.CompletedTask;
	public Task Disconnect() => Task.CompletedTask;
	public Task CloseSubscription(string subscriptionId, CancellationToken token) => Task.CompletedTask;
	public Task PublishEvent(NostrEvent nostrEvent, CancellationToken token) => throw new InvalidOperationException("Synthetic tests never publish.");
	public Task ListenForMessages() => Task.CompletedTask;
	public async IAsyncEnumerable<string> ListenForRawMessages() { await Task.CompletedTask; yield break; }
	public void Dispose() { }
#pragma warning disable CS0067
	public event EventHandler<string>? MessageReceived;
	public event EventHandler<string>? InvalidMessageReceived;
	public event EventHandler<string>? NoticeReceived;
	public event EventHandler<(string subscriptionId, NostrEvent[] events)>? EventsReceived;
	public event EventHandler<(string eventId, bool success, string messafe)>? OkReceived;
	public event EventHandler<string>? EoseReceived;
#pragma warning restore CS0067
}
