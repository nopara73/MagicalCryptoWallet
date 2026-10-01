#!/usr/bin/env python3
"""Build the small, renamed OFL font used by the historical Chinese password mask."""
import argparse
import hashlib
import re
from pathlib import Path
from fontTools import subset
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parents[2]
FONT_SHA256 = '2c76254f6fc379fddfce0a7e84fb5385bb135d3e399294f6eeb6680d0365b74b'
FAMILY = 'MagicalCryptoWallet Password Mask'
POSTSCRIPT = 'MagicalCryptoWalletPasswordMask-Regular'

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('source', type=Path, help='Pinned NotoSansCJKsc-Regular.otf')
parser.add_argument('license', type=Path, help='Original Sans/LICENSE from the same revision')
args = parser.parse_args()
if hashlib.sha256(args.source.read_bytes()).hexdigest() != FONT_SHA256:
    raise SystemExit('The source font does not match the pinned revision.')

source = (ROOT / 'MagicalCryptoWallet.Fluent/Controls/ChinesePasswordTextPresenter.cs').read_text(encoding='utf-8-sig')
sentences = re.findall(r'"([^"\r\n]*[\u3400-\u9fff][^"\r\n]*)"', source)
characters = ''.join(sentences) + ' •'
font = TTFont(args.source, recalcTimestamp=False)
copyright_notice = font['name'].getDebugName(0)
license_text = copyright_notice + '\n\n' + args.license.read_text(encoding='utf-8-sig')
options = subset.Options()
options.name_IDs = ['*']
options.name_legacy = True
options.name_languages = ['*']
subsetter = subset.Subsetter(options=options)
subsetter.populate(text=characters)
subsetter.subset(font)
names = {1: FAMILY, 2: 'Regular', 3: POSTSCRIPT + ';subset-1', 4: FAMILY + ' Regular',
         6: POSTSCRIPT, 13: license_text, 16: FAMILY, 17: 'Regular'}
for record in list(font['name'].names):
    if record.nameID in names:
        font['name'].setName(names[record.nameID], record.nameID, record.platformID, record.platEncID, record.langID)
cff = font['CFF '].cff
cff.fontNames = [POSTSCRIPT]
cff.topDictIndex[0].FamilyName = FAMILY
cff.topDictIndex[0].FullName = FAMILY + ' Regular'
destination = ROOT / 'MagicalCryptoWallet.Fluent/Assets/Fonts'
destination.mkdir(parents=True, exist_ok=True)
font.save(destination / 'PasswordMask-Regular.otf')
(destination / 'PasswordMask.OFL.txt').write_text(license_text, encoding='utf-8', newline='\n')
print(f'Built {len(set(characters))} mask characters; {(destination / "PasswordMask-Regular.otf").stat().st_size} bytes.')
