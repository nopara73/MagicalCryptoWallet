using Avalonia.Media.Imaging;
using FlashCap;
using FlashCap.Utilities;
using FlashCap.Devices;
using System.Linq;
using System.Reactive.Linq;
using System.Runtime.InteropServices;
using SkiaSharp;
using System.Threading;
using System.Threading.Tasks;
using MagicalCryptoWallet.Mcw.Scanning;

namespace MagicalCryptoWallet.Fluent.Models.UI;

public partial class QrCodeReader
{
	public bool IsPlatformSupported =>
		RuntimeInformation.IsOSPlatform(OSPlatform.Linux) ||
		RuntimeInformation.IsOSPlatform(OSPlatform.Windows);

	public IObservable<(string decoded, Bitmap bitmap)> Read()
	{
		return Observable.Create(
			async (IObserver<(string, Bitmap)> result, CancellationToken ct) =>
			{
				var devices = new CaptureDevices();
				var device = devices
					.EnumerateDescriptors()
					.Where(static d => d is not VideoForWindowsDeviceDescriptor)
					.SelectMany(static d => d.Characteristics, static (d, c) => new { d, c })
					.FirstOrDefault() ?? throw new InvalidOperationException("Could not find a device.");

				var tcs = new TaskCompletionSource<object?>(TaskCreationOptions.RunContinuationsAsynchronously);
				using var registration = ct.Register(() => tcs.TrySetResult(default));

				await using var capture = await device.d
					.OpenAsync(
						device.c,
						ct: ct,
						pixelBufferArrived: new PixelBufferArrivedTaskDelegate(async scope =>
						{
							try
							{
								var image = scope.Buffer.ReferImage();
								var decoded = await DecodeCapturedImageAsync(image, ct).ConfigureAwait(false);
								ct.ThrowIfCancellationRequested();
								var bitmap = new Bitmap(image.AsStream());
								try { ct.ThrowIfCancellationRequested(); result.OnNext((decoded, bitmap)); }
								catch { bitmap.Dispose(); throw; }
							}
							catch (OperationCanceledException) when (ct.IsCancellationRequested) { }
							catch (Exception error) { tcs.TrySetException(error); }
						}))
					.ConfigureAwait(false);

				await capture.StartAsync(ct).ConfigureAwait(false);
				await tcs.Task.ConfigureAwait(false);
			});
	}

	internal static async Task<string> DecodeCapturedImageAsync(ArraySegment<byte> image, CancellationToken ct)
	{
		ct.ThrowIfCancellationRequested();
		// Existing capture/image acquisition remains transitional. Only gray
		// pixels cross this boundary; Model 2 detection/text decoding is Rust.
		using var data = SKData.CreateCopy(image.AsSpan());
		using var codec = SKCodec.Create(data) ?? throw new InvalidOperationException("The captured image is invalid.");
		if ((uint)codec.Info.Width is 0 or > 4096 || (uint)codec.Info.Height is 0 or > 4096)
		{ throw new InvalidOperationException("The captured image exceeds the QR frame bounds."); }
		using var bitmap = SKBitmap.Decode(codec) ?? throw new InvalidOperationException("The captured image is invalid.");
		using var gray = bitmap.Copy(SKColorType.Gray8) ?? throw new InvalidOperationException("The captured image cannot be converted to gray pixels.");
		var decoded = await McwQrDecoder.DecodeLuminanceAsync((uint)gray.Width, (uint)gray.Height, (uint)gray.RowBytes, gray.Bytes, ct).ConfigureAwait(false);
		return decoded?.Text ?? "";
	}
}
