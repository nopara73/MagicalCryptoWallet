using Avalonia;

namespace MagicalCryptoWallet.Fluent.Controls.Rendering;

internal record struct DrawPayload(
	HandlerCommand HandlerCommand,
	IDrawHandler? Handler = null,
	Rect Bounds = default);
