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
measures yours. The coupon holds two copies of three lines of test cells:
studs, tube bores and 2x2 pockets, each labelled with its size in millimetres.

```sh
openbrick calibrate coupon --process fdm-0.4 --out coupon.3mf --manifest coupon.json
```

The default coupon is 118.4 x 160.8 x 3.2 mm, about 32 g of PLA, so it fits
the MINI+ bed (180 x 180 mm, with 5 mm kept free at each edge) and of course the
A1 and the XL. The command prints a table of the lines, back to front, with the
value of every cell from left to right; `coupon.json` keeps the same map.

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
  rib depth (for example 6.65 over 0.30). This is the grip a printed tile has
  on a brick below it, so take your time here.

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
# Raised logo, printed face up: two objects, body and accent.
openbrick build parts/ndn.logo-tile.obpart.json --param logo=./my-logo.svg \
  --fit my-fit.json --out tile-emboss.3mf

# Logo cut into the top, one colour.
openbrick build parts/ndn.logo-tile.obpart.json --param logo=./my-logo.svg \
  --param relief=deboss --fit my-fit.json --out tile-deboss.3mf

# Two-colour face printed face down: the file is already turned over.
openbrick build parts/ndn.logo-tile.obpart.json --param logo=./my-logo.svg \
  --param relief=inlay --fit my-fit.json --out tile-inlay.3mf
```

Leave out `--param logo=...` to use the sample mark. Each build validates the
part first: if something would not print or fit (for example no fit profile,
rule `FIT.MISSING`), nothing is written and every problem is listed with its
rule, measured value, threshold and a fix. Warnings never block. Each build
also prints the size, triangle count, plastic and, for two colours, the layer
where the filament change goes. To try things before you calibrate,
`--fit example` uses the bundled example profile: a starting point, not a
calibration of your printer.

## 6. Slice and print the tile

Import the 3MF (PrusaSlicer: File > Import > Import STL/3MF/STEP/OBJ/AMF;
Bambu Studio: File > Import > Import 3MF/STL/STEP; Bambu Studio says the file
is not from Bambu Lab and loads the geometry only, which is all it needs).
The tile is two objects in one frame. When the slicer asks whether to load
objects at several heights as **one object with several parts**, answer
**Yes**, so the logo stays on the tile. Then check the object list: one object
with the parts `ndn.logo-tile body` and `ndn.logo-tile accent`. If your slicer
does not ask and puts the logo on the bed beside the tile, please say so in
your report (with the slicer version); meanwhile a one-colour print still works
from `--out tile.stl` or `--fuse`.

**Emboss (face up).**

- XL: give the body part tool 1 and the accent part tool 2 (object list,
  extruder column, or right-click > Change extruder).
- A1 with AMS lite: in the Objects list, set the filament of each part to the
  AMS slot with that colour.
- MINI+: leave both parts on extruder 1, slice, and in the preview move the
  layer slider to the layer the build named, for example "before layer 17
  (3.20 to 3.40 mm)" with 0.2 mm layers; PrusaSlicer's slider shows that layer
  as 3.40. Click the + next to the slider to add a colour change there. The
  printer stops at that layer and asks for the second filament.

**Deboss** is one colour: print it like the coupon.

**Inlay (face down).** The CLI already turned the tile over, so the face with
the logo is on the bed and the tubes point up; do not flip it. The accent part
is a thin face slab (0.4 mm on a plate-height tile) with the logo cut out of
it, and the body fills in above it, so the logo shows in the body colour,
recessed in the face colour.

- XL and A1: assign the face colour to the accent part and the body colour to
  the body part, as for emboss.
- MINI+: load the face colour first. Add the colour change before the first
  layer the preview shows as body: with a 0.2 mm first layer that is layer 3,
  from 0.40 to 0.60 mm with 0.2 mm layers (slider shows 0.60), or from 0.36 to
  0.52 mm with 0.16 mm layers (slider shows 0.52). Then load the body colour.

Let the print cool before you take it off the bed.

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

If it is too loose, pick a pocket one step tighter (a larger tube OD or a
deeper rib) and record again; if too tight, one step looser. Then build and
pull again.

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
Tile relief mode (emboss, deboss, inlay):
Control pulls (N, three values):
Tile pulls (N, three values):
Median ratio and pass/fail:
Anything odd (warnings, slicer messages, photos):
```

Attach `my-fit.json` too: with your permission it can seed the shared fit
dataset, so the next person with the same printer starts closer.
