using System.IO;
using System.Reactive;
using System.Threading.Tasks;
using Avalonia;
using Avalonia.Controls;
using Avalonia.Media;
using Avalonia.Media.Imaging;
using Avalonia.Metadata;
using ReactiveUI;
using MagicalCryptoWallet.Fluent.Helpers;

namespace MagicalCryptoWallet.Fluent.Controls;

public class QrCode : Control
{
	private const int MatrixPadding = 4;
	private const int MinimumBitmapSizePixelWh = 512;

	public static readonly DirectProperty<QrCode, ReactiveCommand<string, Unit>> SaveCommandProperty =
		AvaloniaProperty.RegisterDirect<QrCode, ReactiveCommand<string, Unit>>(
			nameof(SaveCommand),
			o => o.SaveCommand,
			(o, v) => o.SaveCommand = v);

	public static readonly DirectProperty<QrCode, bool[,]?> MatrixProperty =
		AvaloniaProperty.RegisterDirect<QrCode, bool[,]?>(nameof(Matrix), o => o.Matrix, (o, v) => o.Matrix = v);

	private ReactiveCommand<string, Unit> _saveCommand;

	private bool[,]? _matrix;

	static QrCode()
	{
		AffectsMeasure<QrCode>(MatrixProperty);
	}

	public QrCode()
	{
		UseLayoutRounding = true;
		RenderOptions.SetEdgeMode(this, EdgeMode.Aliased);
		_saveCommand = ReactiveCommand.CreateFromTask<string>(SaveQrCodeAsync);
	}

	private bool[,]? FinalMatrix { get; set; }

	public ReactiveCommand<string, Unit> SaveCommand
	{
		get => _saveCommand;
		set => SetAndRaise(SaveCommandProperty, ref _saveCommand, value);
	}

	[Content]
	public bool[,]? Matrix
	{
		get => _matrix;
		set
		{
			if (SetAndRaise(MatrixProperty, ref _matrix, value))
			{
				FinalMatrix = value is { } ? AddPaddingToMatrix(value) : null;
				InvalidateMeasure();
				InvalidateVisual();
			}
		}
	}

	public async Task SaveQrCodeAsync(string address)
	{
		if (FinalMatrix is null)
		{
			return;
		}

		var file = await FileDialogHelper.SaveFileAsync(
			"Save QR Code...",
			new[] { "png" },
			$"{address}.png",
			Environment.GetFolderPath(Environment.SpecialFolder.MyPictures));
		if (file is null)
		{
			return;
		}

		var path = file.Path.LocalPath;

		if (!string.IsNullOrWhiteSpace(path))
		{
			var ext = Path.GetExtension(path);

			if (string.IsNullOrWhiteSpace(ext) || ext.ToLowerInvariant().TrimStart('.') != "png")
			{
				path = $"{path}.png";
			}

			using var output = File.Create(path);
			SavePng(output);
		}
	}

	/// <summary>Export the displayed symbol with sharp pixels and four-module margins.</summary>
	public void SavePng(Stream output)
	{
		if (FinalMatrix is null) { throw new InvalidOperationException("There is no QR code to export."); }
		var qrCodeSize = GetQrCodeSize(FinalMatrix, Bounds.Size);
		var pixSize = PixelSize.FromSize(qrCodeSize.coercedSize, 1);

		if (pixSize.Width < MinimumBitmapSizePixelWh || pixSize.Height < MinimumBitmapSizePixelWh)
		{
			pixSize = new PixelSize(MinimumBitmapSizePixelWh, MinimumBitmapSizePixelWh);
		}
		// The PNG canvas is an exact multiple of the padded module count.
		var modules = FinalMatrix.GetLength(0);
		var pixels = (int)Math.Ceiling((double)pixSize.Width / modules) * modules;
		pixSize = new PixelSize(pixels, pixels);

		using var rtb = new RenderTargetBitmap(pixSize);
		using (var rtbCtx = rtb.CreateDrawingContext())
		{
			DrawQrCodeImage(rtbCtx, FinalMatrix, pixSize.ToSize(1));
		}

		rtb.Save(output);
	}

	private bool[,] AddPaddingToMatrix(bool[,] source)
	{
		var (indexW, indexH) = GetMatrixIndexSize(source);
		var nW = indexW + (MatrixPadding * 2);
		var nH = indexH + (MatrixPadding * 2);

		var paddedMatrix = new bool[nH, nW];

		for (var i = 0; i < indexH; i++)
		{
			for (var j = 0; j < indexW; j++)
			{
				paddedMatrix[i + MatrixPadding, j + MatrixPadding] = source[i, j];
			}
		}

		return paddedMatrix;
	}

	private (int indexW, int indexH) GetMatrixIndexSize(bool[,] source) =>
		(source.GetUpperBound(0) + 1, source.GetUpperBound(1) + 1);

	private void DrawQrCodeImage(DrawingContext ctx, bool[,] source, Size size, double renderScaling = 1)
	{
		var qrCodeSize = GetQrCodeSize(source, size, renderScaling);
		var (indexW, indexH) = GetMatrixIndexSize(source);
		var gcf = qrCodeSize.gridCellFactor;

		var canvasSize = new Rect(size);

		ctx.DrawRectangle(Brushes.White, null, canvasSize);

		for (var i = 0; i < indexH; i++)
		{
			for (var j = 0; j < indexW; j++)
			{
				var cellValue = source[i, j];
				var rect = new Rect(i * gcf, j * gcf, gcf, gcf);
				var color = cellValue ? Brushes.Black : Brushes.White;
				ctx.DrawRectangle(color, null, rect);
			}
		}
	}

	public override void Render(DrawingContext context)
	{
		var source = FinalMatrix;

		if (source is null)
		{
			return;
		}

		DrawQrCodeImage(context, source, Bounds.Size, TopLevel.GetTopLevel(this)?.RenderScaling ?? 1);
	}

	private (Size coercedSize, double gridCellFactor) GetQrCodeSize(bool[,] source, Size size, double renderScaling = 1)
	{
		var (indexW, indexH) = GetMatrixIndexSize(source);

		var minDimension = Math.Min(indexW, indexH);
		var availMax = Math.Min(size.Width, size.Height);

		var gridCellFactor = Math.Floor(availMax * renderScaling / minDimension) / renderScaling;

		var maxF = Math.Min(availMax, gridCellFactor * minDimension);

		var coercedSize = new Size(maxF, maxF);

		return (coercedSize, gridCellFactor);
	}

	protected override Size MeasureOverride(Size availableSize)
	{
		var source = FinalMatrix;

		if (source is null || source.Length == 0)
		{
			return new Size();
		}

		return GetQrCodeSize(source, availableSize, TopLevel.GetTopLevel(this)?.RenderScaling ?? 1).coercedSize;
	}
}
