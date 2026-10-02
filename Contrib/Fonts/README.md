# Password mask font

The Chinese password box restores the eight sentences from the original control (introduced in commit `ba51fa432afd8fc6fa4b3d7b40421d130ba33e81`; last implementation before removal at `38a92cf05ac09e2b76e5e0f40f58c2f425401b09`). Creation and confirmation use its original fixed sentence. Other passphrase fields shuffle all eight sentences independently of the password and show their prefix as the user types.

Only the text presenter's rendered layout is masked. The bound password, native editing, selection, input composition and reveal button retain their current behavior. Clipboard operations and accessibility values do not expose a hidden passphrase.

`PasswordMask-Regular.otf` is a small subset of [Noto Sans CJK SC Regular](https://github.com/notofonts/noto-cjk/blob/f8d157532fbfaeda587e826d4cd5b21a49186f7c/Sans/OTF/SimplifiedChinese/NotoSansCJKsc-Regular.otf), pinned at revision `f8d157532fbfaeda587e826d4cd5b21a49186f7c`. The original font SHA256 is `2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b`. The bundled subset has been renamed to **MagicalCryptoWallet Password Mask**, retains the original copyright and SIL Open Font License in its metadata, and ships with `PasswordMask.OFL.txt`. The complete source font is not committed.

To rebuild with Python and fonttools 4.61.1, download the font and `Sans/LICENSE` from that pinned revision to a temporary directory, then run:

```sh
python Contrib/Fonts/build-password-mask.py /path/to/NotoSansCJKsc-Regular.otf /path/to/LICENSE
```

The subset includes every mask character so Chinese password fields render on Windows, macOS and Linux without requiring an installed CJK font. It adds no runtime dependency.
