# Using the real LDraw parts in Build mode

Build mode starts with 13 generated bricks, plates and tiles and a baseplate. This
guide adds hundreds of real parts to its palette: you fetch the LDraw Parts Library
yourself, turn the parts you want into a **pack** (an `.obpack` file) with the
`openbrick` command line, and load that pack in the playground or the desktop app.
Everything happens on your own computer; nothing is uploaded.

The parts are drawn by the LDraw community and are licensed per file under
Creative Commons Attribution (CC BY 2.0 or CC BY 4.0). OpenBrick3D does not ship
them, and a pack made from them is yours to use, not ours to redistribute (see
[the licence note](#licence-and-attribution) at the end).

Time: about a minute to import on a laptop, a few seconds to load the pack.

## 1. Get the library

The official library is one zip, `complete.zip`, about 140 MiB (the 2026-10 release
is 146,853,687 bytes). With the command line:

```sh
openbrick catalog fetch --dest ~/ldraw
```

This downloads `https://library.ldraw.org/library/updates/complete.zip` into
`~/ldraw` (it needs `curl`, uses HTTPS only, honours the `HTTPS_PROXY` of your
environment, refuses to download more than 400 MiB (it watches the file grow and
stops curl at the cap, even if the server never says how big the file is), and
never overwrites an
existing `complete.zip`). It then checks the file's SHA-256 against the release this
version of OpenBrick3D was checked against (update 2026-10,
`023083918da2d3697c794fb4267975509584f3611b993095874206a803fa610f`).

LDraw publishes a new release every few months, and the address always serves the
newest, so after the next release the check fails. The command then keeps the
download as `complete.zip.unverified` and prints the file's hash. If you trust that
release (look at the release notes on <https://www.ldraw.org>), run the command
again with `--sha256 <that hash>`: it accepts the file it already has, without
downloading again. You can also download the file by hand from
<https://library.ldraw.org/updates?latest> and skip this step.

## 2. Make a pack

```sh
openbrick catalog import ~/ldraw/complete.zip --out ldraw.obpack
```

`import` takes the zip or the folder that holds `parts` and `p` (an unzipped
`ldraw` folder). It reads about 25,000 part files, leaves out what Build mode
cannot use or should not show, derives each remaining part's mesh, collision boxes
and connectors from the primitives it is made of, and writes the parts that have
connectors. With the 2026-10 library and the defaults, on a debug build of the
command (a release build is several times faster):

| | parts |
|---|---|
| top-level part files scanned | 24,985 |
| left out: subparts, aliases, obsolete files | 9,266 |
| left out: minifigure and other figure parts (categories Minifig, Figure, Constraction) | 4,253 |
| left out: other construction systems (Duplo and the like) | 665 |
| left out: printed, patterned and sticker variants | 6,479 |
| left out: assemblies and variants with longer file names | 443 |
| left out: other categories | 2,334 |
| **eligible parts in the chosen categories** | **1,545** |
| of those, with at least one derived connector | 1,469 (95.1 percent) |
| written to the pack by default (`--limit 600`, every category in turn) | 600, 10.2 MB, 45 seconds |
| written with `--limit 0 --include-unconnected` | 1,526, 21.5 MB, 58 seconds |

Options:

- `--category-filter Brick,Plate,Tile` limits the pack to those LDraw categories
  (case does not matter). The default is Brick, Plate, Tile, Slope, Baseplate,
  Wedge, Arch, Window, Door, Windscreen, Panel, Cylinder, Cone, Fence and Bracket.
  Technic parts (pin holes, axles) are in the category `Technic`: ask for them by
  name. Their pin and axle connectors are derived, but see [what is not done
  yet](#what-is-not-done-yet).
- `--limit N` keeps the first N parts, taking the categories in turn so a small
  limit still has bricks, plates, tiles and slopes; `--limit 0` keeps all that
  qualify. Within a category the parts with plain numbers (`3001`) come before
  suffixed ones (`3001a`), lowest number first. That is a stand-in for "common":
  LDraw has no usage counts. If you want particular parts, name their category and
  raise the limit.
- `--resolution low|standard|high` chooses which primitives round parts use: the
  library's own `p/8` (few segments), `p/` or `p/48` (many) variants. Standard is the
  default; high makes round studs smoother and the pack larger.
- `--include-unconnected` also writes parts for which no connector was found. They
  can only stand on the ground, and the palette says so.
- `--force` replaces an existing output file.

Read the numbers back at any time:

```sh
openbrick catalog info ldraw.obpack
```

## 3. Load it in Build mode

Open the playground or the desktop app, choose **Build**, and press **Add parts from
an LDraw library…** under the parts palette. Choose your `.obpack` (the desktop app
shows its native file dialog). Reading it takes a moment; then:

- The palette gets a **search** box (words, and sizes like `2x4` match "2 x 4"), a
  **category** menu and **pages** of 24 parts. Pictures are drawn from the core's
  meshes only for the page you look at.
- The pack's attribution, the library version, its counts and the full licence text
  show under the palette for as long as the pack is loaded, and each chosen part
  shows its author and licence line.
- Parts are placed like any other: the core snaps studs and receptacles, checks
  collisions and tells you why a part cannot go somewhere.
- **Remove the LDraw pack** goes back to the built-in parts. If the model on screen
  uses pack parts it cannot stay on screen, so it is set aside, not thrown away:
  **Restore previous model** brings it back (it asks for the pack again), and when
  the model has unsaved changes the page asks first (Save first, Replace or Cancel).
  Adding a pack that lacks parts the model uses does the same.

A part is marked in the palette when it is not the usual stud-and-receptacle part:

- **ground only**: no connector was found, so it can only stand on the ground.
- **joint not modelled**: the part has a clip, hinge, ball or socket joint. Its studs
  snap; the joint does not. Joints need a calibration table that is still open
  (the Q04 report).

## 4. Models that use the pack

A saved `.obrick.json` that uses pack parts says so:

```json
"requires": [{ "pack": "ldraw", "library": "2026-10", "fingerprint": "…64 hex digits…" }]
```

Part ids of the pack look like `ld.3001`; the built-in ids (`ob.brick-2x4`) never
change. Opening such a model without the pack loaded does not fail: Build mode shows
what it needs ("This model uses 12 parts from the ldraw pack (library 2026-10)") and
a button to add a pack; when you do, the model opens. A pack of another library
version opens the model too, as long as it has every part the model uses; the page
then shows a notice with both library versions and fingerprints, because the parts'
shapes may differ from the ones the model was built with. The `requires` list is
checked when the model is read (at most 16 entries; `pack` lower-case, `library` up
to 32 printable characters, `fingerprint` 64 lower-case hex digits).

On the command line:

```sh
openbrick bom house.obrick.json --pack ldraw.obpack
```

## Licence and attribution

- The LDraw Parts Library is licensed per file under CC BY 2.0 or CC BY 4.0; the
  licence texts are in the library's `CAreadme.txt`, `CAlicense.txt` and
  `CAlicense4.txt`. This software uses the LDraw Parts Library
  (<https://www.ldraw.org>). LDraw is a trademark of the Estate of James Jessiman.
- A pack is a derived work. It carries the attribution line, those three licence
  texts and every part's author and licence line, and Build mode shows them while
  it is loaded. Keep them with the file.
- **Do not commit a library or a pack to a repository and do not include one in a
  release.** The repository's `.gitignore` already ignores `*.obpack`, `complete.zip`
  and an `ldraw/` folder at its root, so that the tools' output stays out by default.
- The importer leaves out figure parts (the LDraw categories `Minifig...`, `Figure`,
  `Figure Accessory` and `Constraction`, whatever `_` or `~` they carry, and any
  description with "minifig", "mini-doll" or "minidoll"), parts of other
  construction systems and printed variants, even if you name such a category in
  `--category-filter`; OpenBrick3D never draws minifigures. Part names in the palette are the
  descriptions the LDraw authors wrote.
- Downloaded files are treated as untrusted. File sizes, nesting depth, the number
  of references and the zip's entry names are all checked. A zip whose entries
  overlap, whose local headers name other files, whose sizes (or CRC-32) do not
  match or whose entries claim more than 1 GiB in all is refused; one import does
  at most 50 million units of work (references expanded and triangles made; the
  whole default import is 4.7 million) and reads at most 640 MiB, whatever each
  part's own limits allow. A pack is read with limits too (128 MiB file, 128 MiB
  inflated, a length for every text, no control or invisible characters, part boxes
  at most 8,192 LDU wide), and text from a library or a pack is made printable
  before the command line prints it. The importer never writes outside the output
  file. When `import` finds nothing to write it says why (how many parts were
  scanned, failed or too complex, and the first failures).

## What is not done yet

- **Clip, hinge, ball and socket joints** (the calibration table of the Q04 report is
  still open): such parts are flagged and only their studs snap.
- **Pins and axles** are derived and listed (`catalog info` counts them) and the
  engine mates a pin or an axle with a hole when their centres and axes coincide, but
  collision uses each part's box, so a pin cannot yet be pushed into a hole inside
  another part's box. That needs occupancy masks.
- **Slopes, arches and other non-box parts** collide as their bounding boxes; the
  hollow of an arch or the cut-away of a slope cannot take a neighbour yet
  (occupancy masks).
- **Receptacles of large tiles and old tubeless bricks** are partly found (the
  Q04 report's known limit: a 10 x 10 brick without tubes finds 36 of its 100, an
  8 x 16 tile 80 of 128).
- One mesh level per pack, chosen when you import; no chunked culling yet, so very
  large models are limited by the GPU, not by the pack.
- A zip `.obrick` that embeds custom parts.
