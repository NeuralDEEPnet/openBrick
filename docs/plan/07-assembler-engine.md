# Assembler engine

**Purpose.** The data model, lattice and spatial indexes, the snapping algorithm, collision, the connection graph and structural checks, undo/redo and the collaboration path, rendering and performance, desktop versus touch interaction, play mode, build-instruction generation, and save/load. Dimensions and lattice rules come from [04](04-brick-geometry-spec.md); formats from [08](08-file-formats-and-interop.md).

> **Status, 2026-11-08: the assembler alpha, and the LDraw catalog slice of M3a.** The first playable assembler is built on our own generated parts. The LDraw catalog now exists too: our own parser, a bounded resolver over the user's own library (folder or `complete.zip`), connectors derived from primitives, the `.obpack` file, `openbrick catalog import | info | fetch`, and Build mode loads a pack and lists its hundreds of parts with search, categories and pages ([guide](../guides/ldraw-parts.md)). **Large models (2026-11-09):** the engine, the render levels and Build mode now scale to 100,000 parts: stable-slot table, pooled spatial and connector hashes, ray-walk picking, an incremental bill of materials and change list, cached statistics, a stepped loader, delta replies, a four-level render pipeline with chunked InstancedMeshes, frustum culling, distance levels, adaptive resolution, a performance overlay and a `?bench=1` device benchmark (see section 14 for the numbers, measured in software rendering and in the core, and what still needs a real laptop and phone). Still to do from M3a: clip, hinge and ball joints, occupancy masks, 10,000-brick and 100,000-brick frame times on devices ([11](11-roadmap.md), [14](14-lean-ai-agent-development.md) items 21 to 25).
>
> | Section | Built | Not yet |
> |---------|-------|---------|
> | 2 Data model | `openbrick-engine`: struct-of-arrays table (id, part, lattice pose, colour) in placement order; poses are integer LDU plus `Rot24`; ids are small integers, unique per document and never reused after an undo | free poses, flags, steps, submodels, ULIDs |
> | 3 Catalog | `openbrick-catalog`: 13 bricks, plates and tiles and a 16 x 16 baseplate, built by the part evaluator from generated part documents at two levels of detail (24 and 8 segments per stud); the collision box, pick box and connectors (studs, and the receptacle under a body) are derived from the evaluator's features, and a test proves they do not depend on the level of detail; a neutral palette (`profiles/palette/default.json`). **The LDraw pack (2026-11-08):** `catalog::ldraw` (feature `import`, not in the WebAssembly build) has our own parser for line types 0 to 5 with BFC winding (CERTIFY, CW, CCW, INVERTNEXT, the determinant rule; a part that is not certified is written in both orders), a resolver over a folder or the official zip (`parts`, `parts/s`, `p`, `p/48`, `p/8`, with a resolution knob; names are lower-cased, `..` and absolute paths refused; limits on file size, depth, references and triangles; a hostile-input and a randomized no-panic test), tessellation of the library's own primitives to welded f32 meshes, and connectors derived from the primitives each part references as the [Q04 report](../spikes/q04-ldraw-connectivity.md) maps them: studs from `stud*`, pin holes from `peghole` pairs and `beamhole`, axle holes, pins (length from the primitive's own description), axles (length from the matrix); female studs synthesised from the part's triangles (room, cover and grip tests of the report). Collision boxes are the integer-LDU bounds of the part without its studs. Parts are flagged `connectivity: derived` or `none`; parts with clip, hinge, ball or socket primitives carry a `joints` list and only their studs snap. On the 2026-10 library, 1,469 of the 1,545 parts of the default categories (95.1 percent) get a connector, and the receptacle counts of plain `Brick|Plate|Tile W x L` parts match their footprint for 63 of 66 (the three misses are the report's known ones). `.obpack` ([08](08-file-formats-and-interop.md)) holds the curated parts; the engine's catalog is the built-in one plus the pack's parts after it (built-in ids and part numbers stay; pack ids are `ld.<number>`) | clip, hinge, ball and socket joints (the calibration table), the override table for gear holes, clips and towballs, LOD1 and LOD2 hulls, occupancy masks, content-addressed ids, a pack per `.obrick` with several packs |
> | 4 Indexes | spatial hash of pooled chains on 80 x 48 x 80 LDU cells (the 20 x 8 x 20 lattice cell still sizes the box limit), a connector hash keyed on position; a table in stable slots with a separate placement order; at most 150,000 parts | BVH for free poses |
> | 5 Snapping | `Model::snap`: a pointer ray picks a face (top, bottom, side or ground), candidates are the lattice poses in a window of two studs around it, ranked by validity, distance of the contact face to the cursor, mates and offset, a total order; the user's rotation only (the page offers the four yaw turns, the engine takes all 24) | auto-orient, the other yaws and flips as fallbacks, half-stud tolerance for touch |
> | 6 Collision | exact integer box overlap, touching allowed, the body box without its studs (a stud always sits in the cavity of the part above, which is what the stud-vs-cavity rule says for these parts); a pack part collides as the box of its body, which is conservative for slopes, arches and round parts, and keeps a pin from entering a hole inside another part's box | occupancy-mask narrowphase for parts whose shape is not a box |
> | 7 Graph | stud-to-receptacle edges (and pin-in-hole and axle-in-hole edges, which mate by position and axis), union-find components, floating parts (no chain to a part standing on the ground) | articulation points, weak and unstable tiers |
> | 8 Undo | commands with explicit inverses (add, remove, move, turn, recolour, clear, clear built), undo and redo, batches that roll back | drag coalescing, the journal file |
> | 9 Rendering | **(2026-11-09)** four render levels per built-in part (full studs 2,320 triangles for a 2 x 4, cheap studs and tubes 976, solid underside 472, a 12-triangle hull made from the part's own mesh; a pack part has its one mesh and its hull); models up to 3,000 parts are one InstancedMesh per part type (per-instance colour; 14 draw calls at most for the built-ins), bigger ones are chunks of 32 x 32 x 32 studs, frustum culled per chunk and drawn at a level by stud size with hysteresis and a triangle-budget bias, back faces culled, adaptive pixel ratio, only touched chunks re-uploaded, an off-by-default overlay; formerly: one InstancedMesh per placed part type with per-instance colour (14 draw calls at most for the built-ins; with a pack, one per part type in the model, and a mesh is uploaded only for placed parts and the one being placed, the others released beyond a spare of 32), the cheap mesh level above 250 parts, thumbnails drawn on a canvas from the core's meshes for the page of the palette on screen | edges pass, LOD1 without logos (no logos exist yet) |
> | 10 Interaction | pointer ghost, click or tap to place, R and Shift+R, Delete or Backspace, Ctrl or Cmd+Z, Ctrl or Cmd+Shift+Z or Ctrl+Y, Escape, remove mode, buttons for touch; the keyboard path: the focused 3D view takes arrow keys (an aim moved a stud at a time, snapped by the core like a pointer ray), Enter, Shift+arrows to orbit, plus and minus to zoom | marquee, two-finger twist, long-press, eyedropper |
> | 13 Save and load | `.obrick.json`, the plain JSON `model.json` of [08](08-file-formats-and-interop.md) (see the note there; `requires` names the part pack a model needs and a missing pack is an error that says so, not a crash), deterministic text, bill of materials; the reader refuses ids of 0, positions beyond 10^8 LDU and rotations outside 0 to 23; the page keeps a copy of the model so a replaced core worker gets it back (the undo history is lost), asks before Open or a sample replaces unsaved work, and can restore the replaced model | the zip container, `parts/`, thumbnail, journal; `.ldr` and `.mpd` |
>
> The engine's property tests (seeded random cases; `proptest` is not in the dependency tree): every snapped pose is on the fine lattice and its connectors on the stud grid (for the built-in parts; a pack part is held to neither: its connectors mate where they coincide, and a valid snap of a pack part is always accepted by the model); a valid snap never collides and is attached; snapping is deterministic and translation invariant; placing then removing restores the table; undoing any command sequence restores the document text; documents round-trip byte for byte.

## Contents

1. One idea: bricks live on an integer lattice
2. Data model
3. Part catalog
4. Spatial indexes
5. Snapping
6. Collision
7. Connection graph and structural checks
8. Undo, redo, autosave, collaboration path
9. Rendering
10. Interaction: desktop and touch
11. Play mode
12. Build instructions
13. Save and load
14. Complexity summary

## 1. One idea

Canonical state is integer LDU positions plus a 24-element rotation index. Snapping, collision, connectivity, undo, sync and the file format all get cheaper because of it. A tagged free pose (f64 TRS) exists only for parts hinged, clipped or rotated off-lattice. Sideways building needs no special case: five plates (40 LDU) equal two studs (40 LDU); the finest common lattice is 4 LDU, or 2 LDU with half-stud jumpers.

## 2. Data model

Struct-of-arrays tables (ECS-lite), not a deep scene graph.

```rust
struct Instances {
    ulid:      Vec<Ulid>,        // stable id for files and CRDTs
    part:      Vec<PartId>,      // u32 index into the catalog
    pose:      Vec<Pose>,        // enum { Lattice { pos: [i32;3], rot: u8 }, Free(TRS) }
    colour:    Vec<u16>,         // LDraw colour code; palette table maps to RGBA
    flags:     Vec<u8>,          // hidden, locked, selected, mirrored
    step:      Vec<u16>,
    submodel:  Vec<SubmodelId>,
    alive:     Vec<bool>,        // generation-indexed free list
}
```

- Submodels are separate instance tables referenced like MPD subfiles; groups and selections are tag sets.
- Rust: hecs 0.11 or hand-rolled SoA; in the TypeScript bridge, typed-array columns.
- The engine has no renderer types; it exposes column views and change ticks so the viewport updates only dirty chunks.

## 3. Part catalog

Immutable and content-addressed (hash of geometry plus metadata). Per part, baked once and cached:

| Baked datum | Use |
|-------------|-----|
| AABB in LDU | spatial hash insertion, culling |
| Render geometry at three LODs: LOD0 full with optional stud logos (600 to 1,200 tris), LOD1 studs without logos (150 to 300), LOD2 stud-less hull (about 12), plus optional imposter | rendering |
| Mirrored geometry ids pre-baked | BatchedMesh cannot take negative-scale matrices; LDraw allows mirrored placements |
| Occupancy mask at 4 LDU resolution with three classes: solid, stud, cavity (bitsets) | collision narrowphase |
| Connector list in the LDCad SNAP vocabulary (type, gender, pos, dir, sections, DOF, group) | snapping, graph |
| Collision proxy (box set or convex decomposition) | free-pose and custom parts |
| Compatibility class (A to D) | warnings |

Official parts: connectors derived by scanning LDraw primitive references (`stud.dat`, `stud4.dat`, `axlehole.dat`, `peghole.dat`, `clip*.dat`, `clh*.dat`, `phandle*.dat`). LDraw has no bar primitive, and some gear holes, clips, old hinges and towballs are drawn from plain geometry; these come from a CC BY 4.0 override table in the SNAP vocabulary, and contributors refine the rest the same way. Female studs are not primitives: they are synthesised from the part's geometry, as a stud-sized, stud-deep cavity with a wall, tube or pin that grips the stud. Clip and hinge axes come from a per-primitive calibration table ([Q04 spike](../spikes/q04-ldraw-connectivity.md)). Custom parts bring their own connectors from the part designer. The library is downloaded at first run (140 MiB) and packed into a versioned cache; a small CC0 starter set is bundled. **Status 2026-11-08:** the pack is made on the user's machine by `openbrick catalog import` from the library they fetched (`openbrick catalog fetch`, or by hand), not yet by a first-run download in the app; Build mode loads the `.obpack` through a file dialog. The primitive mapping, the female-stud synthesis and the pin, axle and hole pairing follow the Q04 report; the clip, hinge, ball and socket calibration table and the CC BY 4.0 override table are still to do, so those parts are flagged and only their studs snap ([guide](../guides/ldraw-parts.md)).

## 4. Spatial indexes

- **Lattice spatial hash** on 20 x 8 x 20 LDU brick cells using the Teschner hash: `(x * 73856093) xor (y * 19349663) xor (z * 83492791) mod n` with a prime table size, or a packed 64-bit key in an open-addressing map. Each instance is inserted into the cells its AABB covers: O(cells). Teschner's experiments show cell size dominates performance; we size cells to one brick.
- **As built (2026-11-09):** the hash cell is 4 studs by 6 plates by 4 studs (80 x 48 x 80 LDU), entries are 8-byte nodes in pooled chains, boxes are stored with their pick boxes and a ray picks by walking the cells it crosses (stopping at the first cell that cannot hold something nearer); the exact integer boxes still decide every collision. Cell size changes speed, never an answer. **Limits:** at most 150,000 parts and 4,000,000 spatial entries (a flat 256 x 256-stud pack plate covers about 8,450 cells, so about 470 of those; an ordinary model holds 4.6 entries a part), checked before a part is added, moved or loaded so a refused edit changes nothing; a model file of at most 64 MiB (the writer's worst case for 150,000 parts is about 36 MB plus the bill of materials). **Accepted risk:** the hash tables use an unkeyed fast hasher (no host seed exists in the WebAssembly build without a new dependency); a crafted file can make keys collide, which a review simulated at about 1.5 to 2 s per 150,000-key map natively (5 to 10 s over the maps a load fills), bounded by the part and entry caps, and the page opens a document in steps it can cancel.
- **Connector hash** keyed on (quantised position, direction octant, connector type), so a snap query costs O(connectors on the dragged part).
- **BVH** (three-mesh-bvh on the web, parry3d in Rust) only for ray picking, frustum culling and free-pose narrowphase.

## 5. Snapping

```
snap_query(part, cursor_pose):
  candidates = {}
  for c in part.connectors:                      // transformed by the candidate rotation
    for mate in connector_hash.near(c.pos, c.dir.opposite, c.compatible_types):
      pose = pose_that_aligns(c, mate)           // lattice pose
      candidates[pose].count += 1
      candidates[pose].dist  += |pose.pos - cursor.pos|
  rank candidates by (count desc, dist asc)
  for pose in ranked:
    if not collides(part, pose): return pose, mates(pose)
  return nearest lattice pose to cursor with no mates (floating placement allowed, flagged)
```

Rotation handling: the user's current rotation is tried first, then the other three yaw rotations, then flips if "auto-orient" is on. Touch uses a larger snap tolerance (half a stud) and always auto-aligns. Class B and C mates produce warnings in the result. Target cost: under 2 ms at 100k instances.

## 6. Collision

- Broadphase: lattice hash cells overlapped by the candidate AABB.
- Narrowphase for lattice poses: bit-AND of occupancy masks with the class rule: solid vs solid = collision; stud vs cavity = allowed; stud vs solid = collision; cavity vs cavity = allowed.
- Free-pose instances and custom parts without masks: parry3d or three-mesh-bvh triangle tests.
- Never derive collision from the render LOD (the Studio lesson: its collision accuracy depends on the render quality setting).

## 7. Connection graph and structural checks

- Undirected multigraph edges `(instA, connA, instB, connB, type, dof)` in CSR form plus per-instance small vectors.
- Union-find for component membership: near O(1) amortised "is this attached to the ground or baseplate" checks; full component rebuild (BFS, O(component)) only when an articulation vertex is deleted.
- Tarjan articulation points, O(V + E), run incrementally on the affected component after each edit, flag parts hanging on a single connector.
- Three-tier structural report, each flag with an explanation string: **floating** (no path to ground), **weak** (single connector or lever arm beyond N studs), **unstable** (opt-in force-balance LP based on Legolization 2015 and StableLego 2024, run in a worker on the affected subassembly, never in the edit path).

## 8. Undo, redo, autosave, collaboration path

- Command pattern with explicit inverse commands: Place, Remove, Move, Recolour, Regroup, ReorderStep, SetSubmodel; drags coalesce into one transaction.
- The command log is persisted as a journal for autosave and crash recovery; commands address instances by ULID so the same log can later replay into a CRDT.
- No CRDT in v1 (D09). M3b benchmark: load a 100k-instance document into Yjs 13.6.33 (300 KB JS), Loro 1.17.1 (1.20 MB gzip WASM) and Automerge 3.5.0 (1.14 MB gzip WASM); measure load time, memory and update size on a phone. Loro's MovableTree and MovableList map onto submodels and steps; preferred with a Rust core if the numbers hold.

## 9. Rendering

- One API: three.js r186 `WebGPURenderer` with automatic WebGL2 fallback; WebGL2 is the floor.
- Scene structure: chunked `InstancedMesh` per (part, colour, LOD) over 32 x 32 x 32-stud cells so chunks can be frustum-culled and LOD-swapped. `BatchedMesh` (one draw call with multi-draw) only on the WebGL2 backend; on the WebGPU backend it costs one draw per instance as of r186.
- Edges: screen-space normal/depth edge detection (O(pixels)), not per-edge line geometry.
- Stud logos only at LOD0 within a few metres; LOD2 beyond.
- Budgets (from the frameworks research, FLOPS-scaled from a three.js 0.186.1 harness that renders 100k x 6,056-triangle instances at about 20 fps on an RTX 5070 Ti): integrated-GPU laptop 10 to 20 M triangles per frame at 60 fps; mid-range 2024 to 2025 phone 3 to 6 M at 60 fps (6 to 12 M at 30 fps). Hence: 10k bricks at 60 fps everywhere; 100k at 60 fps on laptops with LOD (average under 300 triangles per instance); 30 fps on phones with LOD2, chunk culling and rendering at 0.5 to 0.75 device pixel ratio.
- Week-1 harness (`bench/harness`, written in M0 instead of forking the three.js instancing benchmark): N instanced 2x4 bricks built by the kernel at three LODs (3,620, 548 and 12 triangles), `WebGPURenderer` on WebGPU or forced WebGL2, chunked `InstancedMesh` with per-chunk culling, one CSV row per run (columns, validation rules and phone instructions in `bench/devices/README.md`). Run the standard suite (`?suite=1&device=...`) on an Iris Xe laptop, a Radeon 780M laptop, a base M-series Mac, a Pixel 8a or 9a, a Galaxy A5x (Xclipse) and an iPhone 16e; publish the CSV in `bench/devices/`.

## 10. Interaction: desktop and touch

| Action | Desktop | Touch |
|--------|---------|-------|
| Place | pointer ray to lattice plane; drop | one-finger drag; drop |
| Rotate | R / Shift+R; arrow nudges | two-finger twist on the ghost; rotate button |
| Orbit / zoom | right drag / wheel | two-finger orbit / pinch |
| Select | click; marquee | tap; long-press for multi-select |
| Snap tolerance | quarter stud | half stud; always auto-align |
| Colour | palette panel; eyedropper | bottom sheet palette |
| Undo | Ctrl+Z | two-finger tap; button |

The strongest negative reviews of mobile brick apps are about non-aligning snaps and camera control, so touch mode never produces a floating placement without an explicit "place floating" action.

## 11. Play mode (M5)

Rapier (rapier3d 0.36.1, rapier.js 0.21.0; deterministic WASM, 1.17 MB gzip): one rigid body per connected component with compound box colliders (one box per run of brick cells), revolute joints for hinge connections, prismatic joints for slides, and connection edges broken when contact impulse exceeds a per-connector clutch threshold. Cap simulated bodies (about 2,000) and freeze far components. Never one body per brick at 100k.

## 12. Build instructions

Steps by topological sort of the connection graph constrained by physical support (a part is placed after at least one part it attaches to), grouped by y-layer and by subassemblies found via articulation points, capped at N parts per step, with camera selection that keeps all parts added in the step visible (render-id occlusion test), after Agrawala et al. 2003. Serialised as LDraw `0 STEP` and `0 ROTSTEP` so LPub3D 2.4.9 can consume the output.

## 13. Save and load

Native `.obrick` (zip: `model.json` with semver schema, `parts/` custom part documents or meshes, `thumbnail.png`, `journal.jsonl` optional). LDraw `.ldr`/`.mpd` export is lossless for lattice poses (type-1 lines with integer LDU and the exact 3 x 3 rotation matrix, `0 STEP`, `0 ROTSTEP`, `0 FILE`, `!DATA` for embedded custom parts). Import uses our own type-1 parser resolving part ids and colours from `LDConfig.ldr`; three.js LDrawLoader is used only to build catalog geometry, since it merges parts and ignores `!TEXMAP`. See [08](08-file-formats-and-interop.md).

## 14. Complexity summary

| Operation | Cost |
|-----------|------|
| Insert or move instance in spatial hash | O(cells covered), typically 1 to 4 |
| Snap query | O(connectors x neighbours), independent of model size (0.05 ms at 100,000 parts) |
| Lattice collision test | O(overlapping cells) bit operations |
| Attached check | statistics counted from the table and cached by change tick (about 200 ms at 100,000 parts after a removal; union-find per edit is still to do) |
| Articulation update | O(V + E) of the affected component |
| Render | one draw per (part type, chunk, level) in view (colour is per instance, so it does not split draws); BatchedMesh is not used |
| Undo | O(1) per command; the last 5,000 commands are kept and the page says once when the oldest are discarded |
| Ray pick | walks the spatial hash cell by cell: cost depends on what is near the ray |
| Open | O(parts), in steps; 0.6 s for 100,000 parts |

### Measured performance (2026-11-09)

`bench/scale` ([README](../../bench/scale/README.md)) generates deterministic models (`towers`, `plates` and a seeded random `city`) and measures them. **Core numbers are the Rust code as WebAssembly under node 22 on a 4-CPU machine and are real. Draw calls and triangles per frame are real counts. Frame times in headless Chromium are SwiftShader (software GL), marked "software": they say how fast one CPU rasterises, not how a laptop or phone GPU does.** No real-device number has been taken yet; `?bench=1` ([bench/devices/README.md](../../bench/devices/README.md)) and `bench/harness` are how.

Core, `city` layout (before, at commit 55fadb8: open 19 ms / 71 ms / 149 ms / 472 ms / **8,784 ms**; memory 26 / 26 / 37 / 104 / **413 MiB**; snap 0.13 to **3.7 ms**; pick 0.04 to **3.4 ms**; one edit's state reply 3.5 / 25 / 42 / 127 / **553 ms**; stats 4.5 / 24 / 45 / 137 / **628 ms**, for 1k / 5k / 10k / 25k / 100k bricks):

| bricks | `.obrick` | core open | save | stats (cold) | stats after a removal | edit reply | snap p50 / p95 | pick p50 / p95 | place, remove (mid) + undo | wasm memory |
|---|---|---|---|---|---|---|---|---|---|---|
| 1,000 | 0.09 MB | 15 ms | 2 ms | 0.7 ms | 0.7 ms | 0.23 ms | 0.041 / 0.319 ms | 0.004 / 0.069 ms | 0.01, 0.009 + 0.003 ms | 27 MiB |
| 5,000 | 0.45 MB | 47 ms | 5 ms | 3.7 ms | 3.4 ms | 0.266 ms | 0.038 / 0.085 ms | 0.005 / 0.019 ms | 0.01, 0.01 + 0.003 ms | 27 MiB |
| 10,000 | 0.89 MB | 51 ms | 6 ms | 7.6 ms | 7.3 ms | 0.287 ms | 0.045 / 0.444 ms | 0.007 / 0.254 ms | 0.011, 0.009 + 0.003 ms | 27 MiB |
| 25,000 | 2.26 MB | 150 ms | 41 ms | 31.2 ms | 26.8 ms | 0.443 ms | 0.043 / 0.388 ms | 0.006 / 0.019 ms | 0.011, 0.011 + 0.004 ms | 50 MiB |
| 100,000 | 9.09 MB | 642 ms | 105 ms | 217.2 ms | 254.9 ms | 0.381 ms | 0.064 / 0.888 ms | 0.008 / 0.04 ms | 0.012, 0.019 + 0.007 ms | 195 MiB |

"Edit reply" is what the worker sends after an edit now (the rows that changed and counters); it was the whole table, statistics, bill of materials and the model text (the autosave) before. The statistics are asked for when editing pauses; after a removal the core counts connections again (about 250 ms in WebAssembly at 100,000 parts, 120 ms natively), which is why the page does not ask after every edit. A part placed or removed costs microseconds in the core at every size.

Build mode in headless Chromium, `city` layout, 1280 x 800, **software rendering**:

| bricks | draw calls | triangles / frame | frame time (software) | page open: core + page JS | before: draw calls, triangles, software frame |
|---|---|---|---|---|---|
| 1,000 | 3 | 1.10 M | 1.5 s | 24 + 55 ms | 3, 1.10 M, 1.7 s |
| 5,000 | 22 | 2.68 M | 3.6 s | 30 + 69 ms | 3, 5.41 M, 6.4 s |
| 10,000 | 36 | 5.07 M | 6.1 s | 85 + 100 ms | 3, 10.85 M, 13.3 s |
| 25,000 | 80 | 1.64 M | 2.5 s | 104 + 92 ms | 3, 27.0 M, not timed |
| 100,000 | 243 | 1.20 M | 2.9 s | 635 + 268 ms | 3, 107.8 M, not timed |

The page's place plus undo through the worker fell from 90 ms (10,000) and 237 ms (25,000) to 0.3 and 0.5 ms. The software frame time is dominated by instances and triangles; it is not a prediction. At the default framing a 100,000-brick city is drawn almost entirely as hulls (studs are under 3 pixels), so its frame is 1.2 M triangles; the triangle budget (8 M on a laptop, 2.5 M on a phone) raises the level borders when a frame is over it. The 1,000 to 3,000-brick range is one chunk per part type, drawn at a single level.

**What is still unmeasured:** frame times of 10,000 and 100,000 bricks on an integrated-GPU laptop and a phone, so the budgets above ("10k at 60 fps, 100k at 30 to 60 fps") are neither confirmed nor refuted; the level borders (28, 8 and 3 pixels per stud) and the budgets are first guesses to tune from those rows.
