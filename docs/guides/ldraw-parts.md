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
| of those, with at least one derived connector | 1,477 (95.6 percent; 1,469 before the joint table and the axle fix) |
| written to the pack by default (`--limit 600`, every category in turn) | 600, 10.4 MB, 45 seconds |
| written with `--limit 0 --include-unconnected` | 1,526, 21.5 MB, 58 seconds |

Options:

- `--category-filter Brick,Plate,Tile` limits the pack to those LDraw categories
  (case does not matter). The default is Brick, Plate, Tile, Slope, Baseplate,
  Wedge, Arch, Window, Door, Windscreen, Panel, Cylinder, Cone, Fence and Bracket.
- `--technic` adds the categories Technic, Hinge, Bar and Wheel to the chosen ones
  (off by default): beams, bricks with holes, pins, axles, gears, hinges, clips and
  bars. See [Technic and joints](#technic-and-joints).
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
- **technic**: the part connects by pins or axles (holes, pins, axles).
- **joint**: the part connects by a clip, a bar, a ball, a socket or hinge fingers.
- **joint not calibrated**: the part has a joint primitive the joint table does not
  calibrate (a hinge plate, an odd clip, a bar handle drawn without a bar). Its
  studs, pins and axles snap; the joint does not.

The palette has **filter chips** (All, Technic, Clips and joints) when the pack has
such parts, next to the search and the category menu.

## Technic and joints

```sh
openbrick catalog import ~/ldraw/complete.zip --out technic.obpack --technic --limit 0
```

On the 2026-10 library (the numbers below are from a release build; about 20 seconds):

| | |
|---|---|
| parts of the chosen categories | 2,611 (default categories plus Technic, Hinge, Bar, Wheel) |
| with at least one connector | 2,299 (88.1 percent) |
| with pin or axle connectors | 730 or more (pin holes 1,530, pins 160, axle holes 583, axles 450) |
| with clip, bar, ball, socket or hinge connectors | 232 parts: 153 clip or bar parts (73 clip and 413 bar connectors), 10 with a ball or socket (7 and 5), 71 with a hinge half (31 odd and 67 even connectors) |
| with a joint the table does not calibrate (flagged) | 31 |
| with an occupancy mask | 1,899 (68 million solid cells), 22 more needed too many boxes and keep their box |
| pack file | 35 MB |

What works end to end:

- **Pins and axles.** A pin enters a pin hole in another part, an axle its cross-shaped
  hole, through the part's own shape (occupancy masks, see below): a Technic brick's hole
  row takes a pin even though the pin's box is inside the brick's. Point at a part near a
  hole: the ghost lies the pin down onto the hole's axis (whichever way you had turned
  it), is green when nothing overlaps, and the line above the 3D view says "pin in hole" or
  "axle in hole". A pin spins freely about its axis and an axle fits in four quarter turns;
  the part is kept at the one lattice pose it was placed in, and the statistics count
  the connection as one that leaves a rotation free. R chooses another roll.
- **Clips and bars, balls and sockets, hinges.** A clip grips a bar or bar handle, a ball
  sits in a socket, an odd-fingered hinge half (one or three fingers) meets an
  even-fingered one on a common axis. The table that says where each joint's axis is,
  `standards/joints-v1.json`, is data derived from the library's own primitives; the
  hover line says "clip on bar", "ball in socket" or "hinge fingers interleaved".
  Nothing moves: the rotation a joint allows is recorded, not animated.
- **Occupancy masks.** A part whose box is not its shape (a pin, a beam with holes, a
  slope, an arch, a round brick) is stored with a mask of its solid cells (1 LDU), so two
  parts collide only where their solids overlap by 2 LDU or more. Plain bricks, plates
  and tiles have none and keep the box test. `catalog info` prints how many parts have one.
  The pack format is 2; a version 1 pack still loads, its parts collide as boxes.
- **Statistics.** The Model panel counts the connections by class: studs in
  receptacles, pins in holes, axles in holes, clips on bars, balls in sockets, hinges.

What does not: the axle end of a pin with axle end (Technic Axle Pin 3749 gets its pin module only), half bushes that the library draws as assemblies (4265c is not imported at all), gear wheels (a fifth derive no hole, and gears do not mesh), hinge
plates (`h1`, `h2`) and bar handles drawn without a bar are flagged and do not
connect, towballs and old hinge bricks need an override table that is still to do,
free poses and rotation about a joint's axis are not modelled (the part stays where
it was placed), and a pin or axle that the library draws touching a part by more
than 2 LDU (a friction pin's ribs: a friction pin 2780 and the long pin 6558 are
plain pins to the engine, with two and three modules) is refused where it would be pressed in. Measured
on the real library, a plate's stud fits 94.1 percent of the receptacles, a pin 71.6
percent of the pin holes and an axle 76.3 percent of the axle holes; the refusals
are geometry (a skirt around a receptacle, a gear hub, a blind hole), and the Technic
bricks, beams and liftarms fit all theirs. The numbers come from
`crates/openbrick-engine/tests/technic_real.rs`, which runs when
`OPENBRICK_LDRAW_DIR` names the library (a folder or `complete.zip`); CI does not run it.

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

- **Gear holes, towballs, hinge plates and bar handles drawn without a bar**: the
  override table of the Q04 report is still to do; such parts are flagged or have no
  connector of that kind.
- **Pins and axles, clips and hinges** connect and collide through the occupancy masks;
  rotation about a joint's axis is recorded, not simulated.
- **Slopes, arches and other non-box parts** collide as their shapes (masks); a part
  larger than a 100 LDU cube, or too fragmented (a few gears and wheels), keeps its box.
- **Receptacles of large tiles and old tubeless bricks** are partly found (the
  Q04 report's known limit: a 10 x 10 brick without tubes finds 36 of its 100, an
  8 x 16 tile 80 of 128).
- One mesh level per pack, chosen when you import; the masks do not depend on it.
- A zip `.obrick` that embeds custom parts.
