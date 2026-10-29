# Print test: from the calibration coupon to your own logo tile

This guide takes you from a fresh clone to a printed, brick-compatible 2x4 logo
tile that grips genuine bricks, using only the `openbrick` command line and your
slicer. It is written for FDM printers with a 0.4 mm nozzle and checked against
three setups:

| Printer | Bed | Colours | Slicer |
|---------|-----|---------|--------|
| Prusa MINI+ | 180 x 180 mm | one extruder (filament change by hand) | PrusaSlicer 2.9 |
| Prusa XL (multi-tool) | 360 x 360 mm | one tool per colour | PrusaSlicer 2.9 |
| Bambu Lab A1 with AMS lite | 256 x 256 mm | four filaments | Bambu Studio |

Budget about two hours for the coupon, a few minutes per tile, and half an hour
for the fit check and the pull test. PLA is fine for the first round; the plan
recommends PETG for parts that get played with (docs/plan/05 section 11).

## 1. Install

You need Rust 1.90 or newer ([rustup.rs](https://rustup.rs)) and git.

```sh
git clone https://github.com/NeuralDEEPnet/openBrick
cd openBrick
cargo install --path crates/openbrick --locked
openbrick --version
```

Run the commands below from the `openBrick` folder, so the example part
`parts/ndn.logo-tile.obpart.json` is found. Every command has `--help`.

## 2. Print the calibration coupon

Every printer squeezes plastic a little differently, so the first print
measures yours. The coupon holds two copies of four lines of test cells:
studs, tube bores and two lines of 2x2 pockets (one varies the tube outer
diameter, the other the rib depth), each cell labelled with its size in
millimetres.

```sh
openbrick calibrate coupon --process fdm-0.4 --out coupon.3mf --manifest coupon.json
```

The default coupon is 118.4 x 160.8 x 3.2 mm, about 32 g of PLA, so it fits
the MINI+ bed (180 x 180 mm, with 5 mm kept free at each edge) and of course the
A1 and the XL. The command prints a table of the lines, back to front, with the
value of every cell from left to right; `coupon.json` keeps the same map. On
a bed too short for one copy behind the other, the copies lie side by side,
and the table marks each line `left` or `right`.

On the A1 or the XL you may print the full pocket cross instead (202.4 x
212.8 mm, about 78 g): add `--pockets full --bed 256x256` (A1) or
`--pockets full --bed 360x360` (XL), and record later with
`--pocket-set full`. The default set is enough for a first test.

Slicer settings (the same for the coupon and the tiles):

- **Orientation:** as written. The flat base goes on the bed, labels and cells
  face up. Do not rotate or scale it.
- **Supports:** none. Every cell opens upward.
- **Layers:** 0.2 mm (or 0.16 mm) with a 0.2 mm first layer, the `fdm-0.4`
  profile.
- **Walls:** 3 perimeters (PrusaSlicer: Print Settings > Layers and perimeters >
  Perimeters; Bambu Studio: Process > Strength > Wall loops).
- **Compensation:** elephant-foot compensation 0 and XY size (contour and hole)
  compensation 0, because the first-layer chamfer is already in the model.
  PrusaSlicer (Expert mode): Print Settings > Advanced > Slicing. Bambu Studio:
  Process > Quality.

## 3. Find the cells that fit

Use genuine bricks and plates you trust, at room temperature, and test both
copies of every line.

- **Studs:** press a genuine 1x1 plate onto each stud.
- **Tube bores:** turn a genuine 1x1 plate over and press its stud into each
  bore.
- **2x2 pockets:** turn a genuine 2x2 plate or brick over and press it into each
  pocket. Each pocket label has two numbers: the tube outer diameter over the
  rib depth (for example 6.65 over 0.30). The default coupon prints a cross:
  one line varies the tube OD at rib 0.30, the other varies the rib depth at
  OD 6.50. This is the grip a printed tile has on a brick below it, so take
  your time here.

The best cell presses on with a clear, firm push, holds when you turn the
coupon over and shake it, and comes off by hand without tools. Too loose: it
falls off. Too tight: it needs a lot of force, or the plastic turns white or
cracks. When the two copies disagree by one step, pick the looser of the two.

## 4. Record your fit profile

Type the labels of the best cells. A pocket is one choice: both numbers come
from the same pocket.

```sh
openbrick calibrate record --process fdm-0.4 --pocket-set cross \
  --printer "Prusa MINI+" --material PLA --brand "your brand" \
  --slicer "PrusaSlicer 2.9" \
  --stud 4.80 --tube 4.95 --pocket 6.65/0.30 \
  --out my-fit.json
```

It writes `my-fit.json` and prints each offset (chosen cell minus the
standard size). A value that is not on the coupon, or a pocket your pocket set
did not print, is refused with the list of valid choices. Record one profile
per printer and material; print the coupon again when you change filament
brand.

## 5. Build a logo tile from your SVG

Your artwork should be a single-colour SVG with filled shapes; convert text
and strokes to paths first (Inkscape: Path > Object to Path, Path > Stroke to
Path). Lines thinner than 0.8 mm cannot print and are removed; the build tells
you how much (rule `FEATURE.MIN`).

```sh
# Raised logo, printed face up: one object with two parts, body and accent.
openbrick build parts/ndn.logo-tile.obpart.json --param logo=./my-logo.svg \
  --fit my-fit.json --out tile-emboss.3mf

# Logo cut into the top, one colour.
openbrick build parts/ndn.logo-tile.obpart.json --param logo=./my-logo.svg \
  --param relief=deboss --fit my-fit.json --out tile-deboss.3mf

# Two-colour face for one extruder (MINI+), printed face down: the file is
# already turned over, and the build names the layer for the filament change.
openbrick build parts/ndn.logo-tile.obpart.json --param logo=./my-logo.svg \
  --param relief=inlay --fit my-fit.json --out tile-inlay.3mf

# Two-colour face for an AMS or a tool changer (A1, XL), printed face down:
# the logo fill is its own part, so each part takes its own filament.
openbrick build parts/ndn.logo-tile.obpart.json --param logo=./my-logo.svg \
  --param relief=inlay --param inlayStyle=parts --fit my-fit.json \
  --out tile-inlay-parts.3mf

# The Neuraldeep.net demo tile: the same tile with the Neuraldeep.net mark
# (parts/ndn.brand-badge.obpart.json is the square badge on 4x4).
openbrick build parts/ndn.brand-tile.obpart.json --fit my-fit.json \
  --params relief=inlay inlayStyle=parts --out brand-tile.3mf
```

Leave out `--param logo=...` to use the sample mark. The Neuraldeep.net marks
belong to Neural Deep Network Ltd and may be used for these demo tiles only
([notice](../../parts/assets/brand/NOTICE.md)). In the browser playground the
sample chooser under the drop zone switches between the Neuraldeep.net mark
(its default), the square badge and the CC0 sample mark. Each build validates the
part first: if something would not print or fit (for example no fit profile,
rule `FIT.MISSING`), nothing is written and every problem is listed with its
rule, measured value, threshold and a fix. Warnings never block. Each build
also prints the size, triangle count, plastic and, for two colours, the layer
where a single extruder changes filament (for `inlayStyle=parts` it says
instead that no filament change is needed: the logo and the face around it
share every layer, so that tile needs one filament per part). To try things
before you calibrate, `--fit example` uses the bundled example profile: a
starting point, not a calibration of your printer.

## 6. Slice and print the tile

A two-colour 3MF from `openbrick build` (and from the browser playground) is
**one object whose parts are the two colours**, `ndn.logo-tile body` and
`ndn.logo-tile accent`, in place in one frame, with a display colour for each.
Of the layouts a plain 3MF can have, this is the one the slicers' import code
handles best ([spike Q18](../spikes/q18-slicer-multimaterial-import.md)). The
slicer steps below come from reading that code and the slicers' issue
trackers, not from a print yet: please check them against what you see and
note the differences in your report (section 8).

**Bambu Lab A1 with AMS lite (Bambu Studio).**

1. File > Import > Import 3MF/STL/STEP, or drag the file in. It is not a Bambu
   Studio project, so Bambu Studio loads the geometry without print settings,
   which is all it needs.
2. If a **Standard 3MF Import color** dialog opens, choose **Cancel**. Bambu
   Studio 2.5 and later read the file's display colours into this dialog, and
   users report that it paints the colours onto the surface instead of giving
   each part a filament
   ([bambulab/BambuStudio#9666](https://github.com/bambulab/BambuStudio/issues/9666),
   still open). Not yet confirmed: whether the dialog opens for this file, and
   whether Cancel leaves the per-part filaments working. If it does not, see
   "Without colours" below.
3. Check the Objects list: one object, `Logo tile`, with two parts. Bambu
   Studio names the parts after the object, so the first part, `Logo tile`, is
   the body and the second, `Logo tile_2`, is the logo; tell them apart by
   shape if in doubt. An inlay downloaded from the browser playground is titled
   `Logo tile (face down, as it prints)`, so there the object and its parts
   carry that name (`Logo tile (face down, as it prints)_2` for the second).
4. Set the filament of each part to the AMS slot with its colour (the filament
   column of the Objects list, or right-click the part > Change filament).

**Prusa XL (PrusaSlicer).**

1. File > Import > Import STL/3MF/STEP/OBJ/AMF. PrusaSlicer reads each part as
   an object of its own and then asks **"Multi-part object detected"**: answer
   **Yes**, so the logo stays on the tile as a part of one object.
2. The object is named after the file (for example `tile-emboss`), with the
   parts `ndn.logo-tile body` on extruder 1 and `ndn.logo-tile accent` on
   extruder 2. PrusaSlicer ignores the file's colours, so check the extruder
   column of the object list and change a part's tool if your filaments sit
   on other tools (right-click > Change extruder).

**Prusa MINI+ (PrusaSlicer, one extruder).** Import as for the XL and answer
**Yes** to the same question, leave both parts on extruder 1, and add a colour
change at the layer the build named (below).

If a slicer does not ask and puts the logo on the bed beside the tile, please
say so in your report, with the slicer version; meanwhile a one-colour print
still works from `--out tile.stl` or `--fuse`.

**Emboss (face up).**

- A1 and XL: give the body part the body colour and the accent part the logo
  colour, as above.
- MINI+: slice, and in the preview move the layer slider to the layer the
  build named, for example "before layer 17 (3.20 to 3.40 mm)" with 0.2 mm
  layers; PrusaSlicer's slider shows that layer as 3.40. Click the + next to
  the slider to add a colour change there. The printer stops at that layer and
  asks for the second filament.

**Deboss** is one colour: print it like the coupon.

**Inlay (face down).** The CLI already turned the tile over, so the face with
the logo is on the bed and the tubes point up; do not flip it. There are two
styles.

- **Filament swap** (`--param relief=inlay`, the default style; made for the
  MINI+, and it prints on the XL and the A1 too). The accent part is a thin
  face slab (0.4 mm on a plate-height tile) with the logo cut out of it, and
  the body fills in above it, so the logo shows in the body colour, recessed
  in the face colour.
  - XL and A1: assign the face colour to the accent part and the body colour
    to the body part.
  - MINI+: load the face colour first. Add the colour change before the layer
    the build named. On a plate-height tile with a 0.2 mm first layer that is
    layer 3, from 0.40 to 0.60 mm with 0.2 mm layers (slider shows 0.60), or
    from 0.36 to 0.52 mm with 0.16 mm layers (slider shows 0.52). Then load the
    body colour. When a layer sits exactly on the top of the face slab, the
    build says so and puts the change before that layer, so the floor of the
    logo prints in the body colour.
- **Parts** (`--param relief=inlay --param inlayStyle=parts`; for the A1 with
  AMS lite and the XL). The accent part is the logo fill, set into the face
  (0.6 mm deep on the logo tile), and the body is recessed by exactly that shape, so the
  two touch without overlapping. Give the accent part the logo colour and the
  body part the face colour; no filament change is needed. A single extruder
  cannot print this style, because the logo and the face around it share
  every layer, and its one-colour export (`.stl` or `--fuse`) is the plain tile
  without a visible logo: on the MINI+ use the filament swap.

Let the print cool before you take it off the bed.

**Still to confirm with your print** (Q18). Please note in your report:

- whether the file loads as one object with two parts, and whether the logo
  stays on (emboss) or in (inlay) the face;
- which dialogs appear (Bambu Studio's colour dialog, PrusaSlicer's
  multi-part question) and what you chose;
- which filament or tool each part got, and whether you had to change it;
- whether the sliced preview shows the logo colour only in the logo.

**Without colours.** If the Bambu colour dialog gets in the way, build the
same tile again with `--no-colour` added to your `openbrick build` command,
for example:

```sh
openbrick build parts/ndn.logo-tile.obpart.json --param logo=./my-logo.svg \
  --param relief=inlay --param inlayStyle=parts --fit my-fit.json \
  --no-colour --out tile-inlay-parts-no-colour.3mf
```

The file has the same parts, meshes and orientation, built with your fit
profile and logo, so it is still the tile for the pull test (section 7); only
the display colours are left out. Without them the dialog has nothing to read
and should not open (Q18, from Bambu Studio's source; not yet confirmed by a
print). Give each part its filament as in step 4 for the A1. The browser playground
always writes colours, so use the CLI for this. Please say in your report which
file worked. The `-no-colour.3mf` files in the CI artifact `part-exports` are
only a slicer import check: they carry square test artwork, are built without
a fit profile and keep the design orientation, so do not print them for the
pull test.

## 7. Pull test

The pull test measures how hard the printed tile holds a genuine brick
(docs/plan/05 section 12). You need a force gauge reading 0 to 50 N; a luggage
scale is not accurate enough at 2 to 4 N and is not accepted.

1. **Control first.** Press a genuine 2x4 plate fully onto a genuine 2x4 brick,
   pull it straight up with the gauge and note the force when they separate.
   Do it three times.
2. **Your tile.** Press the printed 2x4 tile fully onto the same genuine 2x4
   brick and pull it straight up the same way, three times.
3. **Result.** Take the middle value (median) of each set of three. The tile
   passes "fits genuine bricks" when its median is between 0.7 and 1.5 times the
   control median.

If it is too loose, pick a pocket one step tighter and record again; if too
tight, one step looser. Then build and pull again. The new pocket must be one
your coupon printed, as its label reads: on the default cross, step along the
line you chose from (a larger or smaller tube OD at rib 0.30, or a deeper or
shallower rib at OD 6.50). `openbrick calibrate record` refuses any other pair
and lists the printed ones. For more choice, print the full pocket set
(`--pockets full`, section 2).

## 8. Share your result

Please report every print test, passed or not, with the **Print test report**
issue form (New issue > Print test report), or paste this:

```text
openbrick --version:
Printer, nozzle:
Material and brand:
Slicer and version, layer height, perimeters:
Process profile and pocket set:
Chosen cells (stud, tube, pocket OD/rib):
Tile relief mode (emboss, deboss, inlay swap, inlay parts):
How the 3MF loaded (one object with two parts? dialogs and your answers? filament or tool per part?):
Control pulls (N, three values):
Tile pulls (N, three values):
Median ratio and pass/fail:
Anything odd (warnings, slicer messages, photos):
```

Attach `my-fit.json` too: with your permission it can seed the shared fit
dataset, so the next person with the same printer starts closer.
