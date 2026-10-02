using Avalonia;
using Avalonia.Controls;
using Avalonia.Media.Imaging;
using Avalonia.Threading;
using Avalonia.VisualTree;
using MagicalCryptoWallet.Fluent.Controls;
using SkiaSharp;
using ZXing;
using ZXing.Common;
using ZXing.QrCode;
using ZXing.SkiaSharp;

internal static class QrChecks
{
    public static void VerifyReceiveFrame(Window window, Bitmap capture, string expected)
    {
        var qr = window.GetVisualDescendants().OfType<QrCode>().Single();
        var matrix = qr.Matrix ?? throw new Exception("The receive QR is missing.");
        var transform = qr.TransformToVisual(window) ?? throw new Exception("Missing receive QR transform.");
        if (Math.Abs(transform.M11 - 1) > 0.000001 || Math.Abs(transform.M22 - 1) > 0.000001 ||
            Math.Abs(transform.M12) > 0.000001 || Math.Abs(transform.M21) > 0.000001)
        { throw new Exception("The receive layout rescaled QR modules."); }
        var origin = qr.TranslatePoint(default, window) ?? throw new Exception("Missing receive QR origin.");
        var scale = window.RenderScaling;
        var left = (int)Math.Round(origin.X * scale); var top = (int)Math.Round(origin.Y * scale);
        if (Math.Abs(origin.X * scale - left) > 0.000001 || Math.Abs(origin.Y * scale - top) > 0.000001)
        { throw new Exception("The receive QR origin is not pixel aligned."); }
        var width = matrix.GetLength(0);
        var pixelWidth = (int)Math.Round(qr.Bounds.Width * scale);
        var pixelHeight = (int)Math.Round(qr.Bounds.Height * scale);
        var cell = Math.Min(pixelWidth, pixelHeight) / (width + 8);
        if (cell == 0) { throw new Exception("The receive QR is too small."); }
        using var output = new MemoryStream(); capture.Save(output);
        using var bitmap = SKBitmap.Decode(output.ToArray());
        using var crop = new SKBitmap();
        if (!bitmap.ExtractSubset(crop, new SKRectI(left, top, left + pixelWidth, top + pixelHeight)))
        { throw new Exception("The receive QR is outside its window."); }
        for (var y = 0; y < crop.Height; y++)
        for (var x = 0; x < crop.Width; x++)
        {
            var color = crop.GetPixel(x, y);
            var (mx, my) = (x / cell - 4, y / cell - 4);
            var dark = mx >= 0 && my >= 0 && mx < width && my < width && matrix[mx, my];
            if (color != (dark ? SKColors.Black : SKColors.White))
            { throw new Exception($"The actual receive QR pixels or four-module margin changed at {x},{y}, scale {scale}."); }
        }
        // A subset shares the full frame's row stride; the decoder adapter expects
        // a tightly packed bitmap. Copy only the symbol into its own pixel storage.
        using var packed = crop.Copy();
        var decoded = new QRCodeReader().decode(new BinaryBitmap(new HybridBinarizer(new SKBitmapLuminanceSource(packed))));
        if (decoded?.Text != expected) { throw new Exception("The actual receive screen QR could not be independently decoded."); }
    }

    public static void Run(string destination)
    {
        var hostPath = Environment.GetEnvironmentVariable("MCW_TEST_EXECUTABLE")
            ?? throw new InvalidOperationException("Set MCW_TEST_EXECUTABLE to the built Rust host.");
        const string text = "bitcoin:bc1qsynthetic?label=你好%20exact";
        var start = new System.Diagnostics.ProcessStartInfo(hostPath)
        {
            RedirectStandardInput = true, RedirectStandardOutput = true, RedirectStandardError = true,
            UseShellExecute = false, CreateNoWindow = true, StandardInputEncoding = new System.Text.UTF8Encoding(false)
        };
        start.ArgumentList.Add("qr"); start.ArgumentList.Add("encode");
        using var process = System.Diagnostics.Process.Start(start)!;
        process.StandardInput.Write(text); process.StandardInput.Close();
        var lines = process.StandardOutput.ReadToEnd().Split('\n', StringSplitOptions.RemoveEmptyEntries);
        process.WaitForExit();
        if (process.ExitCode != 0) { throw new Exception(process.StandardError.ReadToEnd()); }
        var width = int.Parse(lines[0], System.Globalization.CultureInfo.InvariantCulture);
        var matrix = new bool[width, width];
        for (var y = 0; y < width; y++) { for (var x = 0; x < width; x++) { matrix[x,y] = lines[y + 1][x] == '1'; } }
        foreach (var scale in new[] { 1.0, 1.25, 1.5, 2.0 })
        {
            var qr = new QrCode { Matrix = matrix };
            var size = new Size(300 * scale, 300 * scale);
            qr.Measure(size); qr.Arrange(new Rect(qr.DesiredSize)); Dispatcher.UIThread.RunJobs();
            using var output = new MemoryStream();
            qr.SavePng(output);
            var path = Path.Combine(destination, $"receive-qr-{scale:0.00}.png");
            File.WriteAllBytes(path, output.ToArray());
            using var bitmap = SKBitmap.Decode(output.ToArray());
            var pixels = bitmap.Width / (width + 8);
            if (bitmap.Width != bitmap.Height || bitmap.Width % (width + 8) != 0) { throw new Exception("Export scaling is not integral."); }
            for (var y = 0; y < bitmap.Height; y++)
            {
                for (var x = 0; x < bitmap.Width; x++)
                {
                    var color = bitmap.GetPixel(x,y);
                    if (color != SKColors.White && color != SKColors.Black) { throw new Exception("QR export is blurred or transparent."); }
                    var (mx,my) = (x / pixels - 4,y / pixels - 4);
                    var dark = mx >= 0 && my >= 0 && mx < width && my < width && matrix[mx,my];
                    if ((color == SKColors.Black) != dark) { throw new Exception("QR matrix orientation or quiet zone changed."); }
                }
            }
            var decoded = new QRCodeReader().decode(new BinaryBitmap(new HybridBinarizer(new SKBitmapLuminanceSource(bitmap))));
            if (decoded.Text != text) { throw new Exception($"Rendered synthetic QR mismatch: expected {text}, decoded {decoded.Text}."); }
            using var rendered = new RenderTargetBitmap(PixelSize.FromSize(qr.Bounds.Size, 1));
            rendered.Render(qr);
            rendered.Save(Path.Combine(destination, $"receive-qr-screen-{scale:0.00}.png"));
        }
        Console.WriteLine("Receive QR and PNG export: exact independent decoding, pixel alignment, orientation and four-module margins passed.");
    }
}
