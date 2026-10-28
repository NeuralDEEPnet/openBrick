# Spike Q04: connectors from LDraw primitive references

**Question** ([13, Q04](../plan/13-risks-and-open-questions.md#2-open-questions-with-spikes)): how many common LDraw parts get usable connectors (studs, tubes, underside pins, Technic pin holes, axle holes, pins, axles, bars, clips, hinges) only by scanning which LDraw primitive files they reference? Success criterion: at least 70 percent fully correct and 90 percent at least partially correct on 200 common parts; otherwise M3a adds two weeks of hand-authoring ([11](../plan/11-roadmap.md)).

**Answer: yes for the parts we could check.** On the 161 of 200 parts with established ground truth, 93.2 percent are fully correct and 97.5 percent at least partially correct from primitive names alone. Counting the 39 parts without ground truth as failures gives a lower bound of 75.0 percent fully correct and 78.5 percent at least partially correct. The 70 percent gate in [11](../plan/11-roadmap.md) is met even at the lower bound. The 90 percent partial target is met on the verified set; at the lower bound it is not proven, because ground truth is missing, not because failures were seen. **Recommendation: M3a does not need the two extra weeks.** It does need about a week of targeted work, listed at the end.

**Date:** 2026-10-28. **Script:** [`scripts/spikes/ldraw_connectivity.py`](../../scripts/spikes/ldraw_connectivity.py) (Python 3 standard library, MIT). **Part list:** [`scripts/spikes/q04_parts.txt`](../../scripts/spikes/q04_parts.txt). **Results:** [`scripts/spikes/q04_results.csv`](../../scripts/spikes/q04_results.csv).

## Library version

| Item | Value |
|------|-------|
| Source | `https://library.ldraw.org/library/updates/complete.zip`, downloaded 2026-10-28 |
| Size, SHA-256 | 146,853,687 bytes, `023083918da2d3697c794fb4267975509584f3611b993095874206a803fa610f` |
| Latest `!LDRAW_ORG ... UPDATE` tag in `parts/` and `p/` | **2026-10** (newest file date in the zip 2026-10-26; `LDConfig.ldr` UPDATE 2026-05-29) |
| Contents used | `parts/` (24,985 part files, plus 9,321 subparts in `parts/s/`), `p/` (1,790 primitives, plus 928 in `p/48/` and 127 in `p/8/`) |
| Licence | CC BY 2.0 or CC BY 4.0 per file. Nothing from the library is committed: the part list has ids only, and the CSV has ids, our family labels, counts and verdicts |

This software uses the LDraw Parts Library.

## Method

### Part selection

200 part ids in 20 families, fixed in `q04_parts.txt`. The list is hand-curated by the author: common moulds per family, current LDraw ids only (no `~Moved to` aliases, no obsolete files, no prints, no Duplo, no minifigure parts). It is not weighted by set-inventory frequency. Regular families are large (15 bricks, 22 plates, 18 beams), so the list leans towards parts that scanning handles well; the per-family table below shows each family on its own.

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

### Derivation

1. Resolve every type-1 line (`1 colour x y z a b c d e f g h i file`) recursively through `parts/`, `parts/s/`, `p/`, `p/48/` and `p/8/`, case-insensitively, in LDraw search order. Transforms compose as `R = R_parent R_line`, `t = R_parent t_line + t_parent`. Coordinates stay in LDraw LDU with -Y up. Stud groups (`stug*.dat`), subparts and `~Moved to` alias files resolve like any other file.
2. Each primitive in the mapping table below is a connector: the recursion stops there and emits it at the composed transform. Unmapped files are descended into.
3. Post-processing:
   - Point connectors at the same position are deduplicated. Example: 3298 stacks a straight tube and a sloped tube primitive at one tube position.
   - Pin hole openings (`peghole.dat`, two per hole) are paired when they are coaxial, face away from each other and are at most two modules apart.
   - Coaxial segments that overlap or touch are merged into runs. An axle is two bevelled ends plus a perimeter scaled to its length. A 2L pin is two pin segments.
   - Hinge primitives within one module of each other form one hinge.
   - Bar-handle attachments (`phandle*.dat`) are paired into a bar.
4. Counting conventions. Stud, tube, underside pin, clip, ball and socket: one per instance. Pin hole, axle hole and pin: one per module of length, so a 1-module beam hole is 1, a 2L axle joiner is 2 and a 2L pin is 2. Axle and bar: one per continuous shaft, with its length. Hinge: one per joint.
5. Every brick number comes from `standards/dimensions-v1.json` (`ldraw_ldu` of `module`, `plate_height` and `bar_diameter`). Lengths of Technic primitives are written in modules, which is how LDraw names them ("Pin 1.0", "Beam Hole 0.5"). The remaining local offsets were read from the primitive files and are commented in the script. Examples: the bar axis of `phandle1.dat` is at local y = 2, and `axleend2.dat` spans local y from 0 to 2.5.

A secondary mode adds one geometric heuristic, because LDraw has no bar primitive. An outward-facing cylinder primitive (`n-dcyli/cylc/cylo`, at least a half circle) whose radius matches the standard's bar radius (7.95 / 2 LDU, tolerance 0.3) becomes a bar segment. Cylinders inside a `BFC INVERTNEXT` are inner surfaces and are skipped. A run shorter than one plate height is dropped, and so is a run that continues an underside pin.

### Ground truth

- **Rule (51 parts).** Bricks, plates, tiles and baseplates whose description is exactly `Brick|Plate|Tile|Baseplate W x L [x H]`, optionally followed by `with Groove`, `without Understud` or `with Square Corners`. Studs are W x L (tiles 0). A 1-wide part has L - 1 underside pins. Otherwise there are (W - 1)(L - 1) tubes. Baseplates and `without Understud` parts have nothing underneath. These parts also get a position check: stud, tube and pin xz positions must equal the expected grid (either orientation), studs must point up and underside features down.
- **Hand (116 parts, 6 of them with one kind unknown).** A table in the script with a note per entry, written from the physical design named in the description and the author's knowledge of the mould. Examples: beam holes equal the beam length, a 1x4 Technic brick has 3 holes, bracket studs are split into up and side (checked by direction), and plain axles are checked for length in modules. A kind the author could not establish is marked unknown.
- **Unverified (39 parts).** 33 parts have no ground truth at all (slopes with odd undersides, wedges and wings, corner plates, the 1x2 tile's pin). The other 6 are the hand-table parts that match on every known kind, but have one kind unknown, so they cannot be called fully correct. An example is the underside of 1x1 round parts: a tube that sits between four studs, or a bore that only takes one stud.
- **Verdicts.** *Full*: every known kind has the exact count and all position, direction and length checks pass. *Partial*: not full, but at least half of the expected connectors are found and at least half of the derived connectors are expected (recall and precision at least 0.5 by count). *Wrong*: anything else.
- **Caveat.** The same author wrote the mapping and the hand truth, after reading LDraw files. The truth is therefore not blind. A second reviewer with physical parts should re-check the hand table (doc 14 keeps measurement with humans).

**Truth corrections made after the first run (both recorded in the script):**

- **3956.** Bracket 2x2 - 2x2 Up was first written as 8 studs (4 up, 4 side). The LDraw model has a closed top with a centre hole and no top studs. The disagreement cannot be settled without the part, so 3956 is now unverified.
- **63965a.** Bar 6L with thick stop was first written as 1 bar. By the one-run-per-shaft convention it is 2: the stop splits 120 LDU (6 modules) into 96 + 8 (stop) + 16. This affects only the heuristic mode; names-only finds no bar either way.

No other truth entry changed after results were seen.

## Mapping: primitives to connectors

| LDraw primitive files | Connector kind (D06 name) | How it is used |
|-----------------------|---------------------------|----------------|
| `stud`, `studa`, `stud2`, `stud2a` (open), `stud6/6a`, `stud9`, `stud10`, `stud13`, `stud15` (truncated), `stud17/17a`, `stud26`, `studel`, `studh*`, `studp01`, `studx/xa`, `stud-logo*`, `stud2-logo*` | stud (studM) | point at origin, outward local -y |
| `stud4`, `stud4a`, `stud4f*`, `stud4h`, `stud4s/s2`, `stud4o/od/oda`, `stud12` (cross), `stud16/16a/16od`, `stud18a`, `stud21a`, `stud22a`, `stud23/23d` | tube (tubeF) | point, opening along local -y |
| `stud3`, `stud3a` | underside pin of 1-wide parts | point |
| `peghole`, `peghole2`-`6` | pin hole (pinF) | one opening; two are paired into a hole |
| `beamhole`, `connhole`, `connhol2` (1 module), `beamhol2` (half), `connhol3` (one-sided) | pin hole (pinF) | full hole along local y |
| `axlehole`, `axl2hole`, `axl3hole`, `axl4hole`, `axlehol4`, `axlehol5`, `axl2hol8`, `axl3hol8`, `axl5hol8` | axle hole (axleF) | unit length along local y, scaled by the matrix |
| `bush`, `bush0`, `bush0a` | axle hole (axleF) | one module along local z |
| `axle`, `axlehol8` ("Technic Axle Perimeter"), `axleend`, `axleend2`, `axleend2hole` | axle (axleM) | segments merged into one shaft |
| `connect`, `connect2`, `5`-`8`, `10` (1.0), `connect3`, `connect4` (0.5), `confric`, `confric2`, `4`-`12` (1.0), `confric3` (2.0) | pin (pinM) | segments from origin along local -y |
| `clip1`-`clip16` | clip (clipF) | point; the clip axis is **not** calibrated per primitive |
| `clh*` (click-lock hinge), `h1`, `h2`, `arm1`-`arm3` | hinge (hingeFinger) | clustered per joint; no rotation axis or limits |
| `phandle1`, `phandle2` | bar (barM) | attachments paired into a bar |
| `joint8ball`, `joint8socket1`-`3` | ball, socket | point |

`python3 -I scripts/spikes/ldraw_connectivity.py --mapping` prints the exact regular expressions.

Deliberately not mapped:

- Components that are not connectors on their own: `stud2s*` (open-stud extensions), `3-16stud4` (partial tube at cut edges), `npeghol*` (surfaces around holes), `axlehol2/3/6/7/9` and `axl*end` (edges and end faces of axle holes), `connectcollar*`, `connectslit*`, `confricrib*`, `fric*`.
- Families outside this spike: Duplo, Scala, Quatro and Modulex studs, `wpin*` wheel pins, `znap*`, `filstud*`.

**Name rule that matters:** `axlehol8.dat` ("Technic Axle Perimeter") is always a shaft, while `axl2hol8`, `axl3hol8` and `axl5hol8` ("Axle Hole ... Perimeter") are always hole walls. BFC facing cannot tell them apart: in the parts checked (3705 axle, 32064b, 4032a), shafts and hole walls alike are placed with positive determinants and no `INVERTNEXT`.

## Results

| Measure (`python3 -I scripts/spikes/ldraw_connectivity.py <ldraw>`) | Primitive names only | Names + bar-cylinder heuristic |
|---|---|---|
| Fully correct / partial / wrong / unverified (of 200) | 150 / 7 / 4 / 39 | 153 / 6 / 2 / 39 |
| **Over the 161 verified parts:** fully correct | **93.2 %** | 95.0 % |
| at least partially correct | **97.5 %** | 98.8 % |
| wrong | 2.5 % | 1.2 % |
| **Over all 200** (unverified counted as not correct): fully correct | **75.0 %** | 76.5 % |
| at least partially correct | **78.5 %** | 79.5 % |
| wrong / unverified | 2.0 % / 19.5 % | 1.0 % / 19.5 % |

Per family (names only; the last column adds the bar heuristic):

| Family | Parts | Truth rule / hand / unverified | Full | Partial | Wrong | Unverified | Full + heuristic |
|---|---|---|---|---|---|---|---|
| brick | 15 | 15 / 0 / 0 | 15 | 0 | 0 | 0 | 15 |
| brick_modified | 6 | 2 / 2 / 2 | 4 | 0 | 0 | 2 | 4 |
| plate | 22 | 22 / 0 / 0 | 22 | 0 | 0 | 0 | 22 |
| plate_modified | 4 | 0 / 0 / 4 | 0 | 0 | 0 | 4 | 0 |
| tile | 11 | 8 / 0 / 3 | 8 | 0 | 0 | 3 | 8 |
| slope | 14 | 0 / 5 / 9 | 5 | 0 | 0 | 9 | 5 |
| technic_brick | 11 | 0 / 11 / 0 | 9 | 2 | 0 | 0 | 9 |
| technic_beam | 18 | 0 / 18 / 0 | 18 | 0 | 0 | 0 | 18 |
| technic_pin | 10 | 0 / 9 / 1 | 8 | 1 | 0 | 1 | 8 |
| technic_axle | 10 | 0 / 10 / 0 | 10 | 0 | 0 | 0 | 10 |
| technic_connector | 10 | 0 / 8 / 2 | 8 | 0 | 0 | 2 | 8 |
| technic_gear | 4 | 0 / 3 / 1 | 3 | 0 | 0 | 1 | 3 |
| bracket | 9 | 0 / 8 / 1 | 8 | 0 | 0 | 1 | 8 |
| side_studs | 8 | 0 / 8 / 0 | 8 | 0 | 0 | 0 | 8 |
| hinge | 10 | 0 / 10 / 0 | 6 | 3 | 1 | 0 | 6 |
| bar_clip | 12 | 0 / 11 / 1 | 7 | 1 | 3 | 1 | 10 |
| round | 11 | 0 / 9 / 2 | 4 | 0 | 0 | 7 | 4 |
| wedge | 6 | 0 / 0 / 6 | 0 | 0 | 0 | 6 | 0 |
| jumper | 5 | 0 / 4 / 1 | 3 | 0 | 0 | 2 | 3 |
| baseplate | 4 | 4 / 0 / 0 | 4 | 0 | 0 | 0 | 4 |

Per kind, over parts whose truth for that kind is known (names only):

| Kind | Parts expecting it | Expected | Derived | Matched |
|---|---|---|---|---|
| stud | 104 | 5,006 | 5,006 | 5,006 |
| tube | 35 | 428 | 428 | 428 |
| underside pin | 46 | 163 | 163 | 163 |
| pin hole | 34 | 155 | 145 | 145 |
| axle hole | 23 | 33 | 33 | 33 |
| pin | 9 | 14 | 15 | 14 |
| axle | 12 | 12 | 12 | 12 |
| bar | 5 | 6 | 2 (6 with heuristic) | 2 (6) |
| clip | 6 | 7 | 6 | 6 |
| hinge | 10 | 10 | 6 | 6 |
| ball | 1 | 1 | 0 | 0 |

Baseplates contribute most of the studs: the 48x48 baseplate alone has 2,304.

**Library-wide (no ground truth, `--library-stats`):** 10,608 of 20,435 current part files (51.9 %) reference at least one mapped primitive. The parts with none are mostly minifigure parts (3,221), stickers (2,786), figures (639), tiles (568, mostly printed 1x1 and 1x2 tiles, which have no underside feature), Duplo (538) and Technic (265).

**Runtime:** 4.5 s for the 200 parts plus the version scan; about 60 s for the whole library (one core, cold cache).

**Sanity checks run during the spike:**

- Every vertical stud, tube and underside pin lies on the 10 LDU half-grid in x and z.
- In every beam, neighbouring holes are 20 LDU apart and all holes share one axis direction.
- Deliberately breaking the script lowers the score, so the evaluation is not blind to bugs. Disabling the coaxial run merge drops full from 150 to 138; not rotating child translations drops it to 146.

## Failure taxonomy

All 11 names-only failures among verified parts (7 partial, 4 wrong):

| Cause | Parts | What happens |
|---|---|---|
| **Feature drawn from raw geometry; no primitive exists** | 87994 Bar 3L, 63965a Bar 6L (wrong); 2921 Brick 1x1 with handle (partial) | LDraw has no bar primitive; bars are `4-4cyli`/`4-4cylc` at radius 4. The bar heuristic recovers all three. |
| | 15712 Tile 1x1 with clip (wrong) | Clip modelled with `cylo`/`chrd` pieces, not `clip*.dat`. |
| | 6628 towball pin (partial) | Pin found; the ball is sphere geometry. |
| | 3937 Hinge brick base (wrong); 3938 Hinge brick top, 2429 Hinge plate 1x4 base (partial) | Old-style hinges have no hinge primitive; studs and pins are found. |
| **Feature implied by another primitive** | 3709b, 3738 Technic plates with holes (partial) | The vertical pin hole is the bore of the underside tube (`stud4a`); no hole primitive. Rule needed: in a Technic plate with holes, an open tube is also a pin hole. |
| **Kind confusion** | 2430 Hinge plate 1x4 top (partial) | The swivel pivot is drawn with `connect2.dat` (Technic pin) and is derived as a pin, not a hinge. |

Seen in the unverified set, or fixed while building the script. These are the cases the catalog code must handle:

| Pattern | Example | Lesson |
|---|---|---|
| Stacked primitives at one position | 3298: `stud4a` and `stud4s` at the same point | Deduplicate per kind by position, not by position and direction. |
| Ambiguous underside of 1x1 round parts | 6141, 85861, 3062b, 98138 derive one `tube` | The model needs a 1x1 receptacle kind, or a rule from the outer radius. |
| A hollow-stud ring used as a clip | 4081b (clip light) derives 3 studs | Ring holders need per-part semantics. |
| Partial tubes at cut edges | Wings 41769a, 41770a (`3-16stud4`), wedges | Not mapped. Wedge stud layouts are not derivable from the description. |
| Pin extensions drawn as plain cylinders | 3040b: `stud3a` plus a radius-4 cylinder up to the roof | The bar heuristic must attach such cylinders to the underside pin. Without this rule it reported a spurious bar. |
| Mirrored cylinders | 2921 handle, 63965a | Facing must follow `BFC INVERTNEXT` only. A negative determinant reverses winding, but BFC readers compensate for it. Treating it as a flip dropped bar pieces: the full-length half cylinder of the 2921 handle, and the 96 LDU shaft of 63965a. |
| Bar handle anchors | 48336, 2540 | The `phandle*` origin is not on the bar axis (local y = 2), and `phandle2` lets the bar pass through. |

Structural limits that the counts do not show:

1. **Anti-stud receptacles are not primitives.** A 1x1 brick has no underside primitive at all, and the 1x2 tile's underside is plain boxes. Snapping a stud needs a female stud grid under every part. The catalog must synthesise it from the part's footprint (its outer box primitives or bounding box) together with the tube and pin positions. It cannot come from scanning alone.
2. **Clip axes and hinge axes and limits are not in the primitives.** Positions come out right. The clip's bar axis and a click hinge's rotation axis and detent angles need a per-primitive calibration table (about 40 files: `clip1`-`16`, `clh*`).
3. **Lengths come from the matrices, not the names.** Axle holes and axles are unit-length primitives scaled by their matrix, so length and module count are derived, not read.

## Recommendation

- **M3a: no two extra weeks.** The 70 percent gate in [11](../plan/11-roadmap.md) holds at the lower bound (75.0 %) and on the verified set (93.2 %).
- **About one week inside M3a** (estimate) instead, for:
  1. Female stud grid synthesis from footprint plus tubes and pins (algorithmic; covers every brick, plate and tile).
  2. A small CC BY 4.0 override table in the SNAP vocabulary for families without primitives:
     - old hinge bricks and swivel hinge plates (3937, 3938, 2429, 2430 and relatives);
     - bars, unless the bar-cylinder heuristic is adopted (it fixed all 3 bar parts here at no verified regression);
     - clips drawn from raw geometry (15712-type);
     - towballs and ball joints;
     - Technic plates with holes (or the tube-is-a-hole rule);
     - wedges, wings and cut-corner plates.
  3. A calibration table for clip and hinge axes.
  4. A blind second check of the 39 unverified parts by a person with the physical parts.
- **Families that scan well and need no hand work:** bricks, plates, tiles, baseplates, Technic bricks, beams (thick and thin, straight and bent), pins, axles, bushes, cross blocks, angle connectors, gears with axle-hole primitives, brackets, bricks with side studs, click-hinge plates, plates with clips and bar handles, jumpers.
- **What the connector derivation in `openbrick-catalog` must handle:**
  - full transform composition including mirror matrices;
  - recursion through `parts/s/`, `stug*` groups, `p/8/`, and `~Moved to` aliases;
  - stopping at the first mapped primitive;
  - pairing `peghole` openings into holes;
  - merging coaxial segments into shafts, counted per module for holes and pins and per run for axles and bars;
  - deduplicating stacked primitives;
  - anchor offsets for primitives whose origin is off the connector axis (`phandle*`);
  - the `axlehol8` versus `axl*hol8` name rule;
  - facing from `INVERTNEXT` only (for the bar heuristic);
  - female stud grid synthesis;
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
```

`--self-test` needs no library. The plain run (second command) rewrites `scripts/spikes/q04_results.csv`; with the 2026-10 library the file comes out byte-identical (checked by running it twice). A later library update can change the numbers; record the new UPDATE tag when re-running. The download is about 140 MB and stays out of CI.
