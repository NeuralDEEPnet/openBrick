# Neuraldeep.net marks: notice

Copyright Neural Deep Network Ltd. All rights reserved. The Neuraldeep.net marks are trademarks of Neural Deep Network Ltd and are not covered by the licences of this repository (MIT for code, CC0 for parts and data). You may use these files to build, print and show the OpenBrick3D demo tiles; any other use needs written permission.

While a Neuraldeep.net drawing is in use, the OpenBrick3D playground shows the first sentence of this notice under the drop zone, with a link to this file.

## The wordmark drawings

The printable tiles carry the text NEURALDEEP.NET and nothing else. Each file is a single-colour SVG drawing in millimetres, fills only (no text, images, CSS or strokes), sized to the face of the tile it is drawn for inside the tiles' 1 mm margin, so it prints 1:1 there. The letters are the capitals of Inter Display ExtraBold; the corners are rounded to 0.425 mm and the letters spaced at least 0.95 mm apart, so every stroke and every gap is wider than the 0.8 mm minimum feature of the fdm-0.4 process in every relief (raised, pressed in and both two-colour inlays).

| File | Tile | Lines | Capitals |
|------|------|-------|----------|
| `neuraldeep-wordmark-2x6.svg` | 2x6 | NEURAL / DEEP.NET | 5.74 mm |
| `neuraldeep-wordmark-2x8.svg` | 2x8 | NEURALDEEP / .NET | 5.74 mm |
| `neuraldeep-wordmark-2x10.svg` | 2x10 | NEURALDEEP.NET | 6.02 mm |
| `neuraldeep-wordmark-4x4.svg` | 4x4 | NEURAL / DEEP / .NET | 4.92 mm |

On the long tiles the lines run along the tile's length. `neuraldeep-wordmark-2x6.svg` is the default artwork of `parts/ndn.brand-tile.obpart.json` and of the playground, and `neuraldeep-wordmark-4x4.svg` that of `parts/ndn.brand-badge.obpart.json`. The whole text does not print on a smaller tile (2x4 or 2x2): the strokes of E and the counter of A would be narrower than 0.8 mm.

`scripts/brand-text.py` makes these files from the font, which it checks by SHA-256, and `--check` says whether they are current; do not edit them by hand.

### The typeface

The letterforms come from Inter 4.0 (Inter Display ExtraBold, `InterDisplay-ExtraBold.otf`, SHA-256 `7f233cb770bedfb0e4d1d5bada87dade7505513b020ca4b1ae2566fa358263b4`), Copyright 2016 The Inter Project Authors (<https://github.com/rsms/inter>, <https://rsms.me/inter/>), licensed under the SIL Open Font License, Version 1.1 (<https://openfontlicense.org>). The font itself is not in this repository. The OFL covers the font and fonts derived from it, not artwork made with it: "The requirement for fonts to remain under this license does not apply to any document created using the fonts or their derivatives" (OFL preamble; see also [fonts/README.md](../../../fonts/README.md)). These drawings are such artwork, and this notice does not restrict anyone's use of Inter itself.

## The logo image

The playground shows Neural Deep Network Ltd's logo, the illustrated portrait from the header of <https://www.neuraldeep.net/>, unchanged: the same picture, only scaled down and saved as WebP without metadata, with the transparent border trimmed. It is in `apps/web/src/brand/` (`ndn-logo-96.webp` and `ndn-logo-192.webp`, 96 and 192 pixels high), shown in the page header beside the app title and in the footer with the copyright line. The logo is Copyright Neural Deep Network Ltd, all rights reserved, a trademark of Neural Deep Network Ltd, and is covered by this notice like the drawings above; it is never printed.

## Where these files go

The part documents that name the drawings (`ndn.brand-tile`, `ndn.brand-badge`) are CC0 like the other parts; the artwork they use by default is not. The WebAssembly core (`crates/openbrick-wasm`) bundles the drawings as the playground's samples, and the playground and the desktop app built from it carry them and the logo image too. The sample mark `parts/assets/mark.svg` (CC0) stays the default of `parts/ndn.logo-tile.obpart.json`. [TRADEMARK.md](../../../TRADEMARK.md) and [LICENSE-POLICY.md](../../../LICENSE-POLICY.md) describe this exception. For permission to use the marks in other ways, contact Neural Deep Network Ltd through <https://neuraldeep.net>.
