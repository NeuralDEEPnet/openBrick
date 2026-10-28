# Spike Q04: connectors from LDraw primitive references

**Question** ([13, Q04](../plan/13-risks-and-open-questions.md#2-open-questions-with-spikes)): how many common LDraw parts get usable connectors (studs, tubes, underside pins, Technic pin holes, axle holes, pins, axles, bars, clips, hinges) only by scanning which LDraw primitive files they reference? Success criterion: at least 70 percent fully correct and 90 percent at least partially correct on 200 common parts; otherwise M3a adds two weeks of hand-authoring ([11](../plan/11-roadmap.md)).

**Answer: met on the parts we could check, not proven over all 200.**

- **Primitive scanning, strict scoring.** Of the 164 parts with established ground truth, 84.8 percent are fully correct and 95.7 percent at least partially correct. "Fully correct" means right counts, connectors inside the part, lattice axes, and calibrated clip and hinge axes. Over all 200, counting the 36 parts without ground truth as failures, the figures are 69.5 and 78.5 percent. That is one part short of the 70 percent gate, and well short of the 90 percent partial target.
- **Female studs (studF) are not a primitive.** Without them no brick, plate or tile can be snapped onto another. A prototype here synthesises them from the part's triangles. It is right on all 180 sample parts with a known count, but its last rule was fitted on this sample. On 150 held-out library parts it is fully right for 82.0 percent. Adding studF to the primitive scan leaves the full counts unchanged (84.8 percent verified, 69.5 percent lower bound).
- **Clip and hinge axes.** The 11 parts whose clip or hinge position is found, but whose axis is not calibrated, are scored as partial. Accepting them would give 91.5 percent verified and 75.0 percent over all 200.

**Recommendation:** do not add the blanket two weeks of hand-authoring the top 500 parts yet. Before M3a starts, (1) re-check the 36 unverified parts blind with the physical parts and (2) build the clip and hinge calibration table. Then rerun the script. If the lower bound over all 200 is still below 70 percent, add the two weeks. In any case M3a needs about 1.5 weeks (my estimate) of targeted work. The work is listed under [Recommendation](#recommendation).

**Date:** 2026-10-28, revised the same day after review. **Script:** [`scripts/spikes/ldraw_connectivity.py`](../../scripts/spikes/ldraw_connectivity.py) (Python 3 standard library, MIT). **Part list:** [`scripts/spikes/q04_parts.txt`](../../scripts/spikes/q04_parts.txt). **Results:** [`scripts/spikes/q04_results.csv`](../../scripts/spikes/q04_results.csv). The CSV's derived columns are derived from the LDraw Parts Library, update 2026-10, licensed per file under CC BY 2.0 or CC BY 4.0. The attribution is in [`q04_results.csv.license`](../../scripts/spikes/q04_results.csv.license).

## Library version

| Item | Value |
|------|-------|
| Source | `https://library.ldraw.org/library/updates/complete.zip`, downloaded 2026-10-28 |
| Size, SHA-256 | 146,853,687 bytes, `023083918da2d3697c794fb4267975509584f3611b993095874206a803fa610f` |
| Latest `!LDRAW_ORG ... UPDATE` tag in `parts/` and `p/` | **2026-10** (newest file date in the zip 2026-10-26; `LDConfig.ldr` UPDATE 2026-05-29) |
| Contents used | `parts/` (24,985 part files, plus 9,321 subparts in `parts/s/`), `p/` (1,790 primitives, plus 928 in `p/48/` and 127 in `p/8/`) |
| Licence | CC BY 2.0 or CC BY 4.0 per file. Nothing from the library is committed. The part list has ids only. The CSV has ids, our family labels, counts, verdicts and our notes |

This software uses the LDraw Parts Library.

## Method

### Part selection

The list has 200 part ids in 20 families, fixed in `q04_parts.txt`. The author hand-curated it: common moulds per family, current LDraw ids only (no `~Moved to` aliases, no obsolete files, no prints, no Duplo, no minifigure parts). It is not weighted by how often parts appear in sets. The regular families are large (15 bricks, 22 plates, 18 beams), so the list leans towards parts that scanning handles well. The per-family table below shows each family separately, and the library-wide probe gives a view that does not depend on this list.

| Family | Parts | Family | Parts |
|--------|-------|--------|-------|
| brick (1xN, 2xN) | 15 | technic_axle | 10 |
| brick_modified (corner, tall, grille, log, arch) | 6 | technic_connector (bush, joiner, cross block, angle connector) | 10 |
| plate (1xN to 8x16) | 22 | technic_gear | 4 |
| plate_modified | 4 | bracket (SNOT) | 9 |
| tile | 11 | side_studs | 8 |
| slope | 14 | hinge (click hinge, hinge brick, swivel plate) | 10 |
| technic_brick (bricks and plates with holes) | 11 | bar_clip | 12 |
| technic_beam (thick and thin, straight and bent) | 18 | round | 11 |
| technic_pin | 10 | wedge | 6 |
| jumper | 5 | baseplate | 4 |

### Derivation from primitives

1. **Resolve references.** Every type-1 line (`1 colour x y z a b c d e f g h i file`) is resolved recursively through `parts/`, `parts/s/`, `p/`, `p/48/` and `p/8/`, case-insensitively, in LDraw search order. Transforms compose as `R = R_parent R_line` and `t = R_parent t_line + t_parent`. Coordinates stay in LDraw LDU with -Y up. Stud groups (`stug*.dat`), subparts and `~Moved to` alias files resolve like any other file.
2. **Emit connectors.** Each primitive in the mapping table below becomes a connector. The recursion stops there and emits it at the composed transform. Unmapped files are descended into.
3. **Post-process.**
   - Point connectors of one kind at the same position are deduplicated. Example: 3298 stacks a straight tube and a sloped tube primitive at one tube position.
   - Pin hole openings (`peghole.dat`, two per hole) are paired when they are coaxial, face away from each other and are at most two modules apart.
   - Coaxial segments that overlap or touch are merged into runs. An axle is two bevelled ends plus a perimeter scaled to its length; a 2L pin is two pin segments.
   - Hinge primitives within one module of each other form one hinge.
   - Bar-handle attachments (`phandle*.dat`) are paired into a bar.
4. **Count.** Stud, studF, tube, underside pin, clip, ball and socket count one per instance. Pin hole, axle hole and pin count one per module of length, so a 1-module beam hole is 1, a 2L axle joiner is 2 and a 2L pin is 2. Axle and bar count one per continuous shaft, with its length. Hinge counts one per joint.
5. **Brick numbers.** Every brick number comes from `standards/dimensions-v1.json`: the `ldraw_ldu` values of `module`, `plate_height`, `brick_height`, `stud_height`, `stud_diameter`, `side_wall` and `bar_diameter`. Lengths of Technic primitives are written in modules, which is how LDraw names them ("Pin 1.0", "Beam Hole 0.5"). The remaining local offsets were read from the primitive files and are commented in the script. Examples: the bar axis of `phandle1.dat` is at local y = 2, and `axleend2.dat` spans local y from 0 to 2.5.
6. **Bar heuristic (secondary mode).** LDraw has no bar primitive, so a secondary mode adds one geometric rule:
   - An outward-facing cylinder primitive (`n-dcyli/cylc/cylo`, at least a half circle) at the standard's bar radius (7.95 / 2 LDU, tolerance 0.3) becomes a bar segment.
   - Cylinders inside a `BFC INVERTNEXT` are inner surfaces and are skipped.
   - A run shorter than one plate height is dropped, and so is a run that continues an underside pin. That last rule was added after 3040b showed a spurious bar, so it is fitted to this sample.

### Female stud synthesis (prototype)

D06 lists `studF`, the receptacle a stud plugs into, as a connector. LDraw has no primitive for it: a 1x1 brick has no underside primitive at all. `synth_studf` builds studF from the part's triangles:

- **Inputs.** The part is flattened into triangles (all files, mapped primitives included) and bucketed on the xz plane. Candidate cells lie on the stud grid. Both phases, on and between studs, are tried in x and in z, and the phase with the most cells wins.
- **Opening plane.** For each cell, the opening plane is the lowest material in the cell's square.
- **Acceptance.** A cell is a downward studF when three tests pass:
  - **Room.** Nine vertical lines, the centre and eight at stud radius minus 1 LDU, cross no face within stud height above the opening.
  - **Cover.** At least one of those lines meets a face above that height.
  - **Grip.** At least two of sixteen vertical lines 1.5 LDU outside the stud surface cross a face within stud height. That face is a wall, tube or pin that touches the stud.
- **Fitted parameters.** The grip test was added after the first run, because the 1x4 arch got studF under its opening. It uses 16 lines because 8 missed the walls of the 2x2 round parts. Both choices were made on this sample, so the hold-out below is the better measure.
- **Scope.** Only downward receptacles are searched. Side-facing ones, such as the backs of some brackets, are out of scope.

### Ground truth

- **Rule (52 parts).** Bricks, plates, tiles and baseplates whose description is exactly `Brick|Plate|Tile|Baseplate W x L [x H]`, optionally followed by `with Groove`, `without Understud` or `with Square Corners`.
  - Studs are W x L (tiles 0).
  - A 1-wide part has L - 1 underside pins; otherwise there are (W - 1)(L - 1) tubes. Baseplates and `without Understud` parts have nothing underneath.
  - studF is W x L; baseplates have 0, because their underside is flat.
  - Position check: stud, tube, underside pin and studF xz positions must equal the expected grid (either orientation). Studs must point up and underside features down.
- **Hand (118 parts).** A table in the script with a note per entry. I wrote it from the physical design named in the description and my knowledge of the mould.
  - Beam holes equal the beam length. A 1x4 Technic brick has 3 holes.
  - Bracket studs are split into up and side, checked by direction. Plain axles are checked for length in modules.
  - A kind I could not establish is marked unknown. Twelve entries have one unknown kind. Six of them match on every known kind and are therefore unverified; the other six fail on a known kind and are scored on the kinds that are known.
- **studF truth (180 parts).** The 52 rule parts as above. 0 for the 52 Technic beams, pins, axles, connectors and gears. A table of 76 written from the description for the rest: the underside of the part's W x L body, for brackets the horizontal plate only, and 0 for the two bars. Not established: inverted and curved slopes, wedges and wings, cut-corner plates and plates with holes, old hinge tops and swivel plates, 3956, 60474 and 4590.
- **Unverified (36 parts).** 30 parts have no ground truth: slopes with odd undersides, wedges and wings, corner plates. The other 6 are the hand-table parts described above. One example is the underside of 1x1 round parts: either a tube that sits between four studs, or a bore that only takes one stud.
- **Checks on every scored part,** whatever the truth source:
  - every connector position, and both ends of every axial connector, lies inside the part's bounding box (from its triangles, 1 LDU tolerance);
  - every connector axis except clip, hinge, ball and socket is one of the six lattice directions;
  - in Technic beams and bricks, hole axes are parallel and each hole's nearest neighbour is one module away;
  - clip and hinge connectors count as "axis not calibrated". Their position is the primitive origin. For example, `clip2.dat` seats its bar at local z = -10 along local y, not at the origin.
- **Verdicts.**
  - *Full*: every known kind has the exact count and all checks pass.
  - *Partial*: not full, but at least half of the expected connectors are found and at least half of the derived connectors are expected (recall and precision of at least 0.5 by count).
  - *Wrong*: anything else.
- **Caveat.** I wrote the mapping, the hand truth, the studF table and the bar labels myself, after reading LDraw files. None of them is blind. A second reviewer with physical parts should re-check the hand table (doc 14 keeps measurement with humans).

**Truth changes after results were seen, all recorded in the script:**

| Part | Change | Effect |
|------|--------|--------|
| 3956 Bracket 2x2 - 2x2 Up | 8 studs (4 up, 4 side) to unverified. LDraw shows a holed top with no studs; not settled without the part | removed from the verified set |
| 63965a Bar 6L with thick stop | 1 bar to 2: the stop splits 120 LDU into 96 + 8 + 16 | affects the bar-heuristic mode only |
| 3648b Gear 24 with single axle hole | unverified to axle hole 1, other holes unknown. The description names the axle hole | now **wrong** (review finding) |
| 4081b Plate 1x1 with clip light | unverified to clip 1, studs unknown. The ring bore (radius 4 LDU) holds a bar | now **wrong** (review finding); partial if the two ring ends count as studs |
| 3069b Tile 1x2 with groove | the hand override to unverified is removed; the description rule applies (1 underside pin) | now **wrong** (review finding); full if the real tile has no underside pin |
| 15068, 11477 curved slopes | studF 4 and 2 to not established. The 15068 underside is stepped (opening planes at y = 0 and y = -8) | removed from the studF set |

## Mapping: primitives to connectors

| LDraw primitive files | Connector kind (D06 name) | How it is used |
|-----------------------|---------------------------|----------------|
| `stud`, `studa`, `stud2`, `stud2a` (open), `stud6/6a`, `stud9`, `stud10`, `stud13`, `stud15` (truncated), `stud17/17a`, `stud26`, `studel`, `studh*`, `studp01`, `studx/xa`, `stud-logo*`, `stud2-logo*` | stud (studM) | point at origin, outward local -y |
| of those, `stud2`, `stud2a`, `stud6`, `stud6a`, `stud17`, `stud17a`, `stud2-logo*` | stud with `open: true` | the bore (radius 4 LDU) also takes a bar. `stud9` ("with Small Holes", bore radius 2 LDU) is not open in this sense |
| `stud4`, `stud4a`, `stud4f*`, `stud4h`, `stud4s/s2`, `stud4o/od/oda`, `stud12` (cross), `stud16/16a/16od`, `stud18a`, `stud21a`, `stud22a`, `stud23/23d` | tube (tubeF) | point, opening along local -y |
| `stud3`, `stud3a` | underside pin of 1-wide parts | point |
| `peghole`, `peghole2`-`6` | pin hole (pinF) | one opening; two are paired into a hole |
| `beamhole`, `connhole`, `connhol2` (1 module), `beamhol2` (half), `connhol3` (one-sided) | pin hole (pinF) | full hole along local y |
| `axlehole`, `axl2hole`, `axl3hole`, `axl4hole`, `axlehol4`, `axlehol5`, `axl2hol8`, `axl3hol8`, `axl5hol8` | axle hole (axleF) | unit length along local y, scaled by the matrix |
| `bush`, `bush0`, `bush0a` | axle hole (axleF) | one module along local z |
| `axle`, `axlehol8` ("Technic Axle Perimeter"), `axleend`, `axleend2`, `axleend2hole` | axle (axleM) | segments merged into one shaft |
| `connect`, `connect2`, `5`-`8`, `10` (1.0), `connect3`, `connect4` (0.5), `confric`, `confric2`, `4`-`12` (1.0), `confric3` (2.0) | pin (pinM) | segments from origin along local -y |
| `clip1`-`clip16` | clip (clipF) | point at the primitive origin; the bar axis and seat are **not** calibrated |
| `clh*` (click-lock hinge), `h1`, `h2`, `arm1`-`arm3` | hinge (hingeFinger) | clustered per joint; no rotation axis or limits |
| `phandle1`, `phandle2` | bar (barM) | attachments paired into a bar |
| `joint8ball`, `joint8socket1`-`3` | ball, socket | point |
| (no primitive) | studF | synthesised from geometry, see above |

`python3 -I scripts/spikes/ldraw_connectivity.py --mapping` prints the exact regular expressions.

Deliberately not mapped:

- Components that are not connectors on their own: `stud2s*` (open-stud extensions), `3-16stud4` (partial tube at cut edges), `npeghol*` (surfaces around holes), `axlehol2/3/6/7/9` and `axl*end` (edges and end faces of axle holes), `connectcollar*`, `connectslit*`, `confricrib*`, `fric*`.
- Families outside this spike: Duplo, Scala, Quatro and Modulex studs, `wpin*` wheel pins, `znap*`, `filstud*`.

**Name rule that matters:** `axlehol8.dat` ("Technic Axle Perimeter") is always a shaft, while `axl2hol8`, `axl3hol8` and `axl5hol8` ("Axle Hole ... Perimeter") are always hole walls. BFC facing cannot tell them apart: in the parts checked (3705 axle, 32064b, 4032a), shafts and hole walls alike are placed with positive determinants and no `INVERTNEXT`.

## Results

`python3 -I scripts/spikes/ldraw_connectivity.py <ldraw>` prints these numbers. A is the primitive scan; D adds the synthesised studF.

| Measure | A: primitive names, strict | A': axes accepted | B: A + bar heuristic | D: A + studF synthesis |
|---|---|---|---|---|
| Full / partial / wrong / unverified (of 200) | 139 / 18 / 7 / 36 | 150 / 7 / 7 / 36 | 142 / 17 / 5 / 36 | 139 / 22 / 3 / 36 |
| **Over the 164 verified parts:** fully correct | **84.8 %** | 91.5 % | 86.6 % | **84.8 %** |
| at least partially correct | **95.7 %** | 95.7 % | 97.0 % | 98.2 % |
| wrong | 4.3 % | 4.3 % | 3.0 % | 1.8 % |
| **Over all 200** (unverified counted as not correct): fully correct | **69.5 %** | 75.0 % | 71.0 % | **69.5 %** |
| at least partially correct | **78.5 %** | 78.5 % | 79.5 % | 80.5 % |

D's partial figure is lenient. The easy studF count lets a part whose only expected primitive feature is missing reach recall 0.5; for example, 15712 finds 1 of 1 studF and 0 of 1 clip. This lifts four parts from wrong to partial: 3069b, 3937, 15712 and 4081b. A's "at least partial" is the conservative figure.

**Sensitivity of the lower bound.** If the physical 1x2 tile (3069b) has no underside pin, A becomes 140 full, which is exactly 70.0 percent of 200. If the 4081b ring ends count as studs, 4081b moves from wrong to partial.

**Female studs alone (C).**

| Set | Parts | Full | Partial | Wrong |
|---|---|---|---|---|
| Sample (the 200, where the studF count is established) | 180 | 180 (100 %; fitted, see the grip test) | 0 | 0 |
| **Hold-out:** library parts outside the 200 whose description fixes the count (`--studf-holdout`) | **150** | **123 (82.0 %)** | 5 | 22 |
| of which plain `Brick/Plate/Tile/Baseplate W x L` descriptions | 59 | 53 (89.8 %) | 5 | 1 |
| of which `Technic Beam*` (expected 0) | 91 | 70 (76.9 %) | 0 | 21 |

Hold-out failures by cause:

- **Pockets in irregular Technic beams** accepted as receptacles (21 beams). Examples: 2905 triangle liftarm (2 cells), 39370 H-shaped beam (4), 45803 bent double-chamfered beam (11). The catalog needs a rule that keeps studF to parts with a System underside, or a test of the pocket's wall height.
- **Tubeless interiors.** The old 733 Brick 10x10 has no tubes, so its 64 interior cells have nothing within grip reach and only the 36 edge cells pass. This is the price of the grip test that fixed the arch: an interior cell that a stud can enter but nothing touches is a real placement position. The rule must tell "enclosed by distant walls" apart from "open underneath".
- **Large tiles lose cells** (6934 Tile 3x6: 14 of 18; 48288 Tile 8x16: 80 of 128). Not diagnosed.
- **The rule does not fit** the three `Plate W x L x 0.667` parts (15623, 15624, 18922). 15624 has 56 studs, not 64, and an extension with 4 extra cells. These are truth errors more than synthesis errors.

Per family. Truth sources are rule / hand / unverified; studF is full / established.

| Family | Parts | Truth | A full | A partial | A wrong | Unverified | studF | D full | D partial | D wrong |
|---|---|---|---|---|---|---|---|---|---|---|
| brick | 15 | 15 / 0 / 0 | 15 | 0 | 0 | 0 | 15 / 15 | 15 | 0 | 0 |
| brick_modified | 6 | 2 / 2 / 2 | 4 | 0 | 0 | 2 | 6 / 6 | 4 | 0 | 0 |
| plate | 22 | 22 / 0 / 0 | 22 | 0 | 0 | 0 | 22 / 22 | 22 | 0 | 0 |
| plate_modified | 4 | 0 / 0 / 4 | 0 | 0 | 0 | 4 | 1 / 1 | 0 | 0 | 0 |
| tile | 11 | 9 / 0 / 2 | 8 | 0 | 1 | 2 | 11 / 11 | 8 | 1 | 0 |
| slope | 14 | 0 / 5 / 9 | 5 | 0 | 0 | 9 | 9 / 9 | 5 | 0 | 0 |
| technic_brick | 11 | 0 / 11 / 0 | 9 | 2 | 0 | 0 | 11 / 11 | 9 | 2 | 0 |
| technic_beam | 18 | 0 / 18 / 0 | 18 | 0 | 0 | 0 | 18 / 18 | 18 | 0 | 0 |
| technic_pin | 10 | 0 / 9 / 1 | 8 | 1 | 0 | 1 | 10 / 10 | 8 | 1 | 0 |
| technic_axle | 10 | 0 / 10 / 0 | 10 | 0 | 0 | 0 | 10 / 10 | 10 | 0 | 0 |
| technic_connector | 10 | 0 / 8 / 2 | 8 | 0 | 0 | 2 | 10 / 10 | 8 | 0 | 0 |
| technic_gear | 4 | 0 / 4 / 0 | 3 | 0 | 1 | 0 | 4 / 4 | 3 | 0 | 1 |
| bracket | 9 | 0 / 8 / 1 | 8 | 0 | 0 | 1 | 8 / 8 | 8 | 0 | 0 |
| side_studs | 8 | 0 / 8 / 0 | 8 | 0 | 0 | 0 | 8 / 8 | 8 | 0 | 0 |
| hinge | 10 | 0 / 10 / 0 | 0 | 9 | 1 | 0 | 7 / 7 | 0 | 10 | 0 |
| bar_clip | 12 | 0 / 12 / 0 | 2 | 6 | 4 | 0 | 12 / 12 | 2 | 8 | 2 |
| round | 11 | 0 / 9 / 2 | 4 | 0 | 0 | 7 | 10 / 10 | 4 | 0 | 0 |
| wedge | 6 | 0 / 0 / 6 | 0 | 0 | 0 | 6 | 0 / 0 | 0 | 0 | 0 |
| jumper | 5 | 0 / 4 / 1 | 3 | 0 | 0 | 2 | 4 / 4 | 3 | 0 | 0 |
| baseplate | 4 | 4 / 0 / 0 | 4 | 0 | 0 | 0 | 4 / 4 | 4 | 0 | 0 |

Per kind, over parts whose truth for that kind is known (studF synthesised, everything else from primitive names):

| Kind | Parts expecting it | Expected | Derived | Matched |
|---|---|---|---|---|
| stud | 104 | 5,006 | 5,006 | 5,006 |
| studF | 122 | 960 | 960 | 960 |
| tube | 35 | 428 | 428 | 428 |
| underside pin | 47 | 164 | 163 | 163 |
| pin hole | 34 | 155 | 145 | 145 |
| axle hole | 24 | 34 | 33 | 33 |
| pin | 9 | 14 | 14 | 14 |
| axle | 12 | 12 | 12 | 12 |
| bar | 5 | 6 | 2 | 2 |
| clip | 7 | 8 | 6 | 6 (positions only) |
| hinge | 10 | 10 | 6 | 6 (positions only) |
| ball | 1 | 1 | 0 | 0 |

Baseplates contribute most of the studs: the 48x48 baseplate alone has 2,304.

### Library-wide

These figures come from `--library-stats` and have no ground truth.

- **Coverage.** 10,608 of 20,435 current part files (51.9 %) reference at least one mapped primitive. The parts with none are mostly minifigure parts (3,221), stickers (2,786), figures (639), tiles (568, mostly printed 1x1 and 1x2 tiles, which have no underside feature), Duplo (538) and Technic (265).
- **Description probe.** For every current part whose description names a feature, the probe checks whether at least one connector of that kind is derived. LDraw authors wrote the descriptions, not this spike, so this is a blind but noisy recall estimate. A description can name a feature the part only refers to, for example "Bar Holder".

| Feature named in the description | Parts | At least one derived | Recall | Examples missed |
|---|---|---|---|---|
| axle hole | 163 | 139 | 85.3 % | 2994, 30359b, 32198b, 3648a, 3648b |
| pin hole | 209 | 149 | 71.3 % | 11303, 15675, 18590 |
| clip | 198 | 111 | 56.1 % | 15712, 1881, 18896 |
| bar or handle | 429 | 17 | 4.0 % | 11089, 11090 |
| hinge or fingers | 302 | 121 | 40.1 % | 100937, 1015152 (figure leg hinges: probe noise) |
| towball | 31 | 1 | 3.2 % | 2508, 2736, 15456 |
| Technic gear wheel (any hole) | 38 | 30 | 78.9 % | 32198b, 32269, 3648a, 3648b, 4019, 641, 6542a, 6589 |

The gear row answers the review: 8 of 38 gear wheels, among them common ones (3648b Gear 24, 4019 Gear 16, 32269 Gear 20 double bevel, 6589 Gear 12 bevel), have their holes drawn as plain geometry. Gears do not scan reliably and belong in the override table.

### Bar-cylinder heuristic precision

The heuristic fires on 1,912 current parts, against 20 parts with name-based bars. `--bar-sample 50` takes the first 50 hits by SHA-256 of the part id; I labelled each from its description and the printed bar positions, with labels in the script.

- 18 parts are real bars (shield grips, sword hilts, antennas, named bars).
- 20 are not: figure neck posts and hip pins, hinge pins inside complete assemblies, train-wheel stub axles, a towball neck.
- 12 cannot be decided from the description.

**Precision is 47.4 percent of decided labels, between 36 and 60 percent overall.** The heuristic is not good enough to replace hand-authored bar entries. Its result on the three bar parts in the sample (all recovered) was tuned on those same parts and says nothing about precision.

### Runtime

| Run | Time (one core) |
|---|---|
| The 200 parts with geometry and studF, plus the version scan | 9 s |
| `--library-stats` | 70 s |
| `--bar-sample 50` | 60 s |
| `--studf-holdout` | 22 s |

### Checks that the evaluation catches bugs

`--self-test` needs no library. Besides the pure logic, it now builds a three-level synthetic library in a temporary folder, using rotations that do not commute (90 degrees about z, then about x) and offsets written in modules. It checks the composed stud position and direction and the axle hole's centre, axis and length against arithmetic worked out in the docstring. It also runs studF synthesis on synthetic 1x1, 1x3 and arch shells.

| Deliberate bug | Self-test | A full (baseline 139) |
|---|---|---|
| Compose in the wrong order (`R_line R_parent`) | fails | 135 (32034 and 32014 put an axle hole at z = +-38.5, outside the part) |
| Child translation not rotated | fails | 130 |
| Segment axis not rotated | fails | 108 |
| Coaxial runs not merged | fails | 127 |
| Peg-hole openings not paired | fails | 137 |
| studF grip test off | fails (arch fixture) | 139 (D: 138, the arch gets 4 studF) |

Other checks:

- Every vertical stud, tube, underside pin and synthesised studF lies on the 10 LDU half-grid in x and z.
- No two connectors of one kind share a position.
- No derived connector of a calibrated kind has an off-lattice axis.

## Failure taxonomy

All 25 non-full verified parts under A (18 partial, 7 wrong):

| Cause | Parts | What happens |
|---|---|---|
| **Axis not calibrated** | clips 4085c, 60897, 61252, 63868, 60470a; click hinges 44301a, 44302a, 60471, 30383, 4275b, 4276b (11, partial) | Counts and positions are right, but the connector is the primitive origin with no bar axis or rotation axis. A calibration table of about 40 primitives fixes this (A' column). |
| **No primitive for the feature: bars** | 87994 Bar 3L, 63965a Bar 6L (wrong); 2921 Brick 1x1 with handle (partial) | Bars are `4-4cyli`/`4-4cylc` at radius 4. The heuristic recovers these three but has low precision library-wide (above). |
| **No primitive: gear holes drawn as raw geometry** | 3648b Gear 24 (wrong); library-wide 8 of 38 gear wheels, including 4019, 32269, 6589 | The centre axle hole is built from `1-4cyli`/`rect` pieces in subparts (`s/3648s02.dat`), not `axlehole.dat`. |
| **No primitive: clips from raw geometry** | 15712 Tile 1x1 with clip (wrong) | Modelled with `cylo`/`chrd` pieces, not `clip*.dat`. |
| **No primitive: old hinges and towballs** | 3937 Hinge brick base (wrong); 3938, 2429 (partial); 6628 towball pin (partial) | No hinge or ball primitive; studs and pins are found. |
| **Ring holder drawn with stud primitives** | 4081b clip light (wrong) | The ring bore holds a bar (clipF), but LDraw draws the ring ends with two opposite `stud2a` (open stud) primitives, so the scan reports two side studs and no clip. |
| **Feature implied by another primitive** | 3709b, 3738 Technic plates with holes (partial) | The vertical pin hole is the bore of the underside tube (`stud4a`). Rule needed: in a Technic plate with holes, an open tube is also a pin hole. |
| **Kind confusion** | 2430 Hinge plate 1x4 top (partial) | The swivel pivot is drawn with `connect2.dat` (Technic pin) and comes out as a pin, not a hinge. |
| **Description rule versus LDraw model** | 3069b Tile 1x2 (wrong) | The rule expects one underside pin like the other 1xN tiles; LDraw models none. Settle it with the physical part. |

Seen in the unverified set or in the hold-out, or fixed while building the script. These are the cases the catalog code must handle:

| Pattern | Example | Lesson |
|---|---|---|
| Stacked primitives at one position | 3298: `stud4a` and `stud4s` at the same point | Deduplicate per kind by position, not by position and direction. |
| Ambiguous underside of 1x1 round parts | 6141, 85861, 3062b, 98138 derive one `tube` | The model needs a 1x1 receptacle kind, or a rule from the outer radius. |
| Partial tubes at cut edges | Wings 41769a, 41770a (`3-16stud4`), wedges | Not mapped. Wedge stud layouts are not derivable from the description. |
| Pin extensions drawn as plain cylinders | 3040b: `stud3a` plus a radius-4 cylinder up to the roof | The bar heuristic must attach such cylinders to the underside pin. |
| Mirrored cylinders | 2921 handle, 63965a | Facing must follow `BFC INVERTNEXT` only. A negative determinant reverses winding, but BFC readers compensate for it. Treating it as a flip dropped the 2921 handle's full-length half cylinder and the 96 LDU shaft of 63965a. |
| Bar handle anchors | 48336, 2540 | The `phandle*` origin is not on the bar axis (local y = 2), and `phandle2` lets the bar pass through. |
| Covered but ungripped cells | 1x4 arch 3659 | studF needs a grip test, not just room and cover. |
| Stepped undersides | 15068 curved slope | studF opening planes at two heights; the receptacle height is per cell. |
| Pockets and tubeless interiors | Technic beam 2905, Brick 10x10 733 | See the hold-out; the grip rule needs a System-underside guard and an "enclosed by walls" exception. |

## Recommendation

- **Gate status.** The 70 percent fully correct gate in [11](../plan/11-roadmap.md) is met on the 164 parts with ground truth (84.8 percent strict, 91.5 percent with clip and hinge axes accepted). Over all 200 it is not proven: the strict lower bound is 69.5 percent, one part short. The 90 percent partial target of [13](../plan/13-risks-and-open-questions.md) is met on the verified set (95.7 percent) and not over all 200 (78.5 percent). In both cases the shortfall comes from the 36 parts without ground truth, not from failures seen.
- **Do not add the blanket two weeks of hand-authoring the top 500 parts yet.** The failures are concentrated in named families and in two missing pieces of work: female studs and clip and hinge axes. Bricks, plates, tiles, baseplates, Technic bricks, beams, pins, axles, brackets and side studs give 110 of the 139 fully correct parts, so hand-authoring 500 parts would mostly redo parts that already scan correctly.
- **Decision rule.** Before M3a starts, re-check the 36 unverified parts and 3069b blind with the physical parts (about a day), and build the clip and hinge calibration table (a few days). Then rerun the script. If the lower bound over all 200 is still below 70 percent, add the two weeks.
- **Targeted work inside M3a,** about 1.5 weeks (my estimate):
  1. **Female stud synthesis** in `openbrick-catalog`, starting from the prototype here. It must also fix the hold-out failure modes: Technic-beam pockets, tubeless interiors, large tiles, stepped undersides.
  2. **A calibration table for clip and hinge axes** and seats, and hinge limits: `clip1`-`16`, `clh*`, `h1`/`h2`, `arm1`-`arm3`.
  3. **A small CC BY 4.0 override table** in the SNAP vocabulary for families without primitives:
     - bars and bar handles drawn as cylinders (do not adopt the bar heuristic);
     - gear wheels whose holes are raw geometry (3648b, 4019, 32269, 6589 and relatives);
     - old hinge bricks and swivel hinge plates (3937, 3938, 2429, 2430 and relatives);
     - clips drawn from raw geometry (15712 type) and ring holders (4081b);
     - towballs and ball joints;
     - Technic plates with holes (or the tube-is-a-pin-hole rule);
     - wedges, wings and cut-corner plates.
  4. **Open studs** flagged as bar receptacles (`open: true` here).
- **Families that scan well and need no hand work:**
  - bricks, plates, tiles and baseplates (pending the 1x2 tile pin), Technic bricks, and jumpers;
  - Technic beams (thick and thin, straight and bent), pins, axles, bushes, cross blocks and angle connectors;
  - brackets and bricks with side studs.

  Clip plates and click-hinge plates scan for position only. Gears do **not** scan well: 21 percent of gear wheels library-wide derive no hole.
- **What the connector derivation in `openbrick-catalog` must handle:**
  - full transform composition including mirror matrices;
  - recursion through `parts/s/`, `stug*` groups, `p/8/`, and `~Moved to` aliases;
  - stopping at the first mapped primitive;
  - pairing `peghole` openings into holes;
  - merging coaxial segments into shafts, counted per module for holes and pins and per run for axles and bars;
  - deduplicating stacked primitives;
  - anchor offsets for primitives whose origin is off the connector axis (`phandle*`, `clip*`, `clh*`);
  - the `axlehol8` versus `axl*hol8` name rule;
  - facing from `INVERTNEXT` only (for any cylinder rule);
  - female stud synthesis with room, cover and grip tests;
  - bounding-box and lattice-axis checks on every derived connector;
  - output in the D06 connector types, serialisable to LDCad `SNAP_CYL`, `SNAP_CLP` and `SNAP_FGR` metas.

## Reproduce

```
mkdir -p /tmp/ldraw-q04/zip /tmp/ldraw-q04/lib
curl -L -o /tmp/ldraw-q04/zip/complete.zip https://library.ldraw.org/library/updates/complete.zip
unzip -q /tmp/ldraw-q04/zip/complete.zip 'ldraw/parts/*' 'ldraw/p/*' -d /tmp/ldraw-q04/lib
python3 -I scripts/spikes/ldraw_connectivity.py --self-test
python3 -I scripts/spikes/ldraw_connectivity.py /tmp/ldraw-q04/lib/ldraw
python3 -I scripts/spikes/ldraw_connectivity.py /tmp/ldraw-q04/lib/ldraw --explain 3001
python3 -I scripts/spikes/ldraw_connectivity.py /tmp/ldraw-q04/lib/ldraw --library-stats --csv - --json /tmp/ldraw-q04/q04.json
python3 -I scripts/spikes/ldraw_connectivity.py /tmp/ldraw-q04/lib/ldraw --studf-holdout
python3 -I scripts/spikes/ldraw_connectivity.py /tmp/ldraw-q04/lib/ldraw --bar-sample 50
```

The plain run (second command) rewrites `scripts/spikes/q04_results.csv`. With the 2026-10 library the file comes out byte-identical; I checked this by running it twice. A later library update can change the numbers and the bar sample; record the new UPDATE tag when re-running. The download is about 140 MB and stays out of CI.
