using Avalonia;
using Avalonia.Skia;

namespace MagicalCryptoWallet.Fluent.Controls.Rendering;

internal interface IDrawHandler : IDisposable
{
	void Draw(ISkiaSharpApiLease skia, Rect bounds);
}
