# Brick geometry specification (Dimensional Standard v1)

**Purpose.** The normative dimensions for brick-compatible parts, in millimetres and LDraw units, with sources and confidence; the coordinate conventions; the feature catalogue with connection rules; what governs clutch; and the print-target columns that the fit profiles in [05](05-3d-printing-pipeline.md) start from. Code loads these numbers from `standards/dimensions-v1.json` (CC0); no crate hard-codes them.

## Contents

1. The two generating numbers
2. Nominal dimension table
3. Coordinate conventions and the lattice
4. Feature catalogue and connection rules
5. What governs clutch
6. Print targets and compensation starting values
7. Compatibility classes
8. Measurement program
9. How the spec is encoded and tested
10. Sources

## 1. The two generating numbers

Everything derives from the module **M = 8.0 mm** (stud pitch) and the stud **Ø4.8 mm**. The 1958 patent (US 3,005,282) defines the underside by tangency, not millimetres: the hollow tube under a 2-wide part touches the projected circles of four studs, so tube OD = 8 x sqrt(2) minus 4.8 = **6.514 mm**, and its bore equals the stud diameter; the solid pin under a 1-wide part touches two studs, so pin OD = 8 minus 4.8 = **3.2 mm**, which is the **3.18 mm bar family**. The vertical unit is the plate, **3.2 mm**; a brick is three plates, 9.6 mm; five plates equal two studs (40 LDU), which is why sideways building closes.

## 2. Nominal dimension table

Confidence: V = verified from a primary source or two independent measurements; L = likely (community consensus); I = inferred.

| Feature | Nominal (mm) | LDU | Measured / notes | Conf. |
|---------|--------------|-----|------------------|-------|
| Module (stud pitch) | 8.000 | 20 | 7.986 ± 0.002 at 25 °C (Munafo / LUGNET) | V |
| Brick height (no studs) | 9.600 | 24 | 9.582 measured; 3 plates | V |
| Plate height | 3.200 | 8 | 3.194 measured; 5 plates = 2 studs | V |
| Half-plate (bracket arm) | 1.600 | 4 | neck brackets about 3 LDU (1.2 mm) | L |
| Half-stud (jumper) offset | 4.000 | 10 | | V |
| Stud diameter | 4.800 | 12 | 4.87 to 4.90 measured on genuine parts (clutch interference) | V |
| Stud height | 1.700 | 4 (1.6) | 1.6 to 1.8 by source; LDraw models 4 LDU; model 1.7 for print | L |
| Horizontal undersize | 0.100 per side | 0.25 | 1x1 body is 7.8 mm; n-stud length is 8n minus 0.2; no vertical undersize | V |
| 2x4 body | 31.8 x 15.8 x 9.6 | 79.5 x 39.5 x 24 | 31.9 x 15.9 micrometer survey | V |
| Side wall | 1.200 (+0.3 ribs) | 3 | older refs 1.5 = wall + rib; LDraw usually 4 LDU | L |
| Roof (top) thickness | 1.000 | 2.5 | cavity depth brick about 8.6, plate about 2.2 | L |
| Baseplate thickness | 1.300 | 3.25 | thin plates without tubes; verify | I |
| Tube OD (2-wide) | 6.514 | 16.3 (LDraw 16) | 6.48 CMM; LDraw stud4 = 16 LDU = 6.4 | V |
| Tube bore (anti-stud) | 4.800 | 12 | equals stud diameter (patent claim 2) | V |
| Tube wall | 0.857 | 2.1 | (6.514 minus 4.8) / 2 | V |
| Solid pin under 1-wide parts | 3.180 | 8 (approx) | "fractionally less than the gap between two studs" | V |
| Bar diameter | 3.180 | 7.95 (LDraw 8) | official "3.2 shaft"; antennas, blades, minifig accessories | V |
| Hollow stud bore / clip opening | 3.200 | 8 | 3.18 + 2 x 0.01 play | L |
| Technic pin hole | 4.800 | 12 | about 4.90 measured; same as tube bore | V |
| Technic counterbore | Ø6.2 x 0.9 deep | 15.5 x 2.25 (LDraw 16 x 2) | pin collar 0.9 mm long | L |
| Technic hole axis above brick bottom | 5.800 | 14.5 (LDraw 14 = 5.6) | 0.2 mm above centre so studs fit beneath | L |
| Technic hole pitch | 8.000 | 20 | beams one module wide (7.8 body) | V |
| Technic pin shaft / collar | Ø4.8 / Ø6.2 x 0.9 | 12 / 15.5 | friction pins about 4.85 to 4.9 effective; weakest-sourced row | I |
| Axle cross | 4.8 across tips, 1.8 arm | 12, 4.5 (LDraw 4) | 4.75 measured across; turns in a 4.9 round hole | L |
| Axle hole (cross) envelope | 4.8 | 12 | LDraw slot half-width 2 LDU (1.6); real about 1.8 to 1.9 | L |
| Bush / gear hub length | 8.0 (7.8 old) | 20 | 0.1 mm ridges on new bushes | L |
| Small ball (towball, Mixel) | 5.9 | 14.75 (LDraw 16) | official "3.2 shaft w/ 5.9 ball" | V |
| Large ball (Technic, CCBS) | 10.2 | 25.5 | LDraw socket fits Ø10.25 | V |
| Click hinge detent | 22.5° (16 positions) | n/a | fingers about 1.8 mm (dual) / 2.8 mm (single); disc Ø6.5 | L |
| Finger hinge (1978 style) | no public standard | n/a | use LDraw geometry, then measure | I |
| DUPLO scale | 2x: pitch 16, brick 19.2, stud Ø9.6 hollow | 40 / 48 / 24 | preschool Mega is not DUPLO-compatible | V |
| LDU | 0.4000 | 1 | 0.3993 ± 0.0001 measured | V |

Mould and part tolerance of the originals (for honesty in marketing and validators): machines 10 µm, moulds up to 20 µm, about 18 rejects per million (LEGO Company Profile 2010); CMM study (Lemeš 2019) found genuine parts within about 32 to 41 µm of nominal with 4 µm roundness scatter, versus 124 to 160 µm for unbranded clones. The "0.002 mm" figure repeated online has no primary source; never print it.

## 3. Coordinate conventions and the lattice

- **Internal model units:** integer LDU (`i32`) for lattice positions; `f64` millimetres for geometry generation. Conversion `mm = ldu x 0.4` happens only at the file boundary and in the evaluator.
- **Frame:** LDraw's right-handed frame with **minus-Y up**. The origin of a part is centred on its top stud group with the stud bases at y = 0; a brick body spans y = 0 to +24, a plate 0 to +8, studs rise to minus 4; tubes point +Y; hinge parts put the origin at the rotation point. Renderers and exporters flip to Y-up (glTF) or Z-up (3MF, STL) at export.
- **Lattice:** the finest common lattice for studs and plates is **4 LDU** (gcd of 20 and 8); with half-stud jumpers it is **2 LDU**. Snapping steps: 20 LDU in X and Z, 8 LDU in Y (plate), 4 LDU (half-plate, brackets), 10 LDU (jumper).
- **Rotations:** the 24 lattice-preserving cube rotations, stored as a `u8` index with a 24 x 24 composition table. Hinged, clipped or bar-mounted parts use a free pose (f64 TRS) and contribute no lattice connectors while off-lattice.
- **Slightly illegal connections:** the Technic hole axis sits 0.2 mm above the system grid (5.8 mm, LDraw 5.6). The assembler flags such mates as "off-system by 0.2 mm" rather than silently forcing them.

## 4. Feature catalogue and connection rules

Each feature carries geometry, an LDU footprint, gender, LDCad SNAP sections and print rules. Connectors are derived from features; see [06](06-part-designer-and-dsl.md) for the document syntax.

| Feature | Gender | Geometry (nominal) | Mates with | SNAP serialisation |
|---------|--------|--------------------|------------|--------------------|
| `stud` (solid, hollow, open) | M | Ø4.8 x 1.7, top chamfer 0.2 to 0.3; hollow bore Ø3.2 | `tube.field`, `antistud`, Technic hole (Class B) | `SNAP_CYL [gender=M] [secs=R 6 4]` |
| `tube.field` / `antistud` | F | tubes Ø6.514/4.8 at 2x2 centres; Ø3.18 pins at 1-wide mid-points; ribs 0.3 on walls | `stud` | `SNAP_CYL [gender=F] [caps=one] [secs=R 6 8] [grid=...]` |
| `technic.hole` | F | Ø4.8 through, counterbore Ø6.2 x 0.9 both sides, axis 5.8 above bottom | `pin`, `axle` (free turning), `stud` (loose) | `SNAP_CYL [gender=F] [secs=R 6 20]` |
| `pin` | M | Ø4.8 shaft, collar Ø6.2 x 0.9, slotted end | `technic.hole` | `SNAP_CYL [gender=M] [secs=R 6 20]` |
| `axle` | M | cross 4.8 across, 1.8 arms | `axle.hole`, `technic.hole` (turns) | `SNAP_CYL [gender=M] [secs=A 6 20]` |
| `axle.hole` | F | cross envelope 4.8, slot 1.8 | `axle` | `SNAP_CYL [gender=F] [secs=A 6 8]` |
| `bar` | M | Ø3.18 | `clip`, hollow stud, minifig hand | `SNAP_CYL [gender=M] [secs=R 4 L]` |
| `clip` | F | opening 3.2, LDCad default radius 4 LDU length 8 | `bar` | `SNAP_CLP [radius=4] [length=8]` |
| `hinge.finger` | n/a | 22.5° detents, finger widths per table above | opposing finger set | `SNAP_FGR [seq=...]` |
| `ball` / `socket` | M / F | Ø5.9 or Ø10.2 | matching socket / ball | `SNAP_GEN [bounding=sphere ...]` |

Connection rules the engine enforces: stud into tube bore or anti-stud pocket (lattice mate, 1 DOF none); stud into Technic hole (loose, flagged Class B); pin into Technic hole (rotational DOF); axle into axle hole (none) or Technic hole (rotational); bar into clip (rotational about bar axis, slide along bar); finger hinges (rotational in 22.5° steps); balls in sockets (free rotation, limited cone).

## 5. What governs clutch

- Diametral interference of about **0.05 to 0.10 mm** between a 4.87 to 4.90 mm stud and the 4.8 mm bore or wall-rib pocket; the **walls, not the studs, are the spring**.
- Rib engagement about 0.3 mm; tube bore and pin OD on the underside; stud OD on top.
- Separation force of two genuine 2x4 bricks is only about **2.5 to 3 N** (Kollsker and Malaguti 2021 via the Strathclyde 2024 paper). Material shrinkage of 0.1 to 0.8 percent on an 8 mm module is 0.01 to 0.06 mm, the same order as the interference, which is why calibration per material is mandatory.

## 6. Print targets and compensation starting values

These are starting values for `FitProfile`; the calibration coupon overrides them per printer and material ([05](05-3d-printing-pipeline.md)).

| Quantity | FDM 0.4 mm nozzle target | Resin (MSLA) | Notes |
|----------|--------------------------|--------------|-------|
| Printed stud OD | 4.75 to 4.85 mm | nominal x 1.009 to 1.011 scale | outer-contour offset minus 0.10 to minus 0.18 mm |
| Printed anti-stud bore | 4.90 to 5.00 mm | scale as above | hole offset plus 0.05 to plus 0.10 mm |
| Horizontal Technic hole | nominal plus about 0.2 mm, polyhole or slight ellipse | | prints undersize and oval |
| Pitch | 7.98 to 8.02 mm | | |
| Plate height | 3.18 to 3.22 mm | | |
| Walls | 0.8 or 1.2 mm (multiples of a 0.4 mm line) | 0.5 supported / 1.0 unsupported | 1.1 mm prints hollow on a 0.4 nozzle (Strathclyde) |
| Roof | at least 1.0 mm | | bridging over tubes |
| Wall-to-roof fillet | 2 mm | | improved bridge anchoring 335 percent (Strathclyde) |
| Stud-top and bottom-edge chamfer | 0.2 to 0.3 mm | | also absorbs elephant foot |
| Elephant-foot compensation | about 0.2 mm | | PrusaSlicer recommendation for 0.4 nozzle |
| Layer height | 0.16 to 0.2 mm (0.1 mm "detail" profile with 0.2 nozzle) | 0.05 | |
| Orientation | studs up, no supports | studs up, supported underside | |
| PELA per-material tweaks (radius, knob / socket / axle) | PLA minus 0.08 / plus 0.04 / plus 0.07; PETG plus 0.04 / plus 0.10 / plus 0.04; ABS fine minus 0.18 / plus 0.16 / plus 0.14; Nylon minus 0.02 / plus 0.15 / plus 0.06 | | method reused, numbers re-derived by our coupon |
| Shrinkage | PLA 0.2 to 0.3 %, PETG 0.1 to 0.2 %, ABS 0.5 to 0.8 %, ASA 0.4 to 0.6 % | | |

## 7. Compatibility classes

| Class | Connectors | Behaviour across brands | UI treatment |
|-------|------------|-------------------------|--------------|
| A | studs, anti-studs, bars, clips | robust on all 8 mm-grid brands | no warning |
| B | Technic pins, axles, bushes | brand- and material-sensitive (Cobi and Gobricks tighter, Mega inconsistent) | warn when mixing brands or printing without a Class B calibration row |
| C | ball joints, click hinges | brand-specific (Qman balls differ; Mixel vs towbar socket clutch differs) | warn; offer clutch presets "soft (original-like)" and "firm (Cobi/Gobricks-like)" |
| D | DUPLO-scale vs preschool Mega | incompatible | separate scale profile |

## 8. Measurement program

Early in M0 to M1, measure 20 to 30 samples each of: 2x4 brick, 1x2 plate, 1x2 Technic brick, 2L pin (friction and frictionless), 3L axle, 1x1 clip, towball, click-hinge brick. Digital micrometer and calipers; one CMM or µCT session if a partner lab is available. Publish the raw CSV under CC0 in `standards/measurements/`. This resolves the open rows above (pin shaft, axle arm, counterbore 6.2 vs 6.4, hole height 5.8 vs 5.6, hinge fingers, ball sockets) and gives the project a citable dataset nobody else has published.

## 9. How the spec is encoded and tested

- `standards/dimensions-v1.json` (CC0): for every feature, `nominal_mm`, `ldraw_ldu`, `print_target_mm` by process, `confidence`, `source`. Semver; additive within a major.
- `openbrick-units` loads it at build time (include_str) and exposes typed constants; a unit test asserts the derived relations (tube OD = 8 x sqrt 2 minus stud; pin = 8 minus stud; 5 plates = 2 studs).
- Golden-mesh tests in `openbrick-part` generate a 2x4 brick, a 2x4 tile and a 1x2 Technic brick and assert volume, bounding box and feature counts against this table within 0.01 mm.
- The calibration coupon test asserts that the sweep cells differ by exactly the step (0.05 mm) in the generated mesh.

## 10. Sources

Robert Cailliau's brick dimension page; LUGNET / Munafo stud-pitch measurement; US patent 3,005,282; LDraw.org file format 1.0.2 and primitives reference; cfinke/LEGO.scad (MIT) parameter file; PELA Blocks calibration tables (CC BY-SA, method only); Lemeš 2019 (SN Applied Sciences 1:193) CMM study; Air and Wodehouse, Strathclyde, DESIGN 2024 ("Play well, print well"); LEGO Company Profile 2010 via Wikipedia for mould tolerance; Brickipedia and brickarchitect for Technic and hinge geometry; Eurobricks threads for pin collar and bush lengths. Full URLs are in the research handoff files referenced from [13](13-risks-and-open-questions.md#sources).
