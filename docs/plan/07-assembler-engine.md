# Assembler engine

**Purpose.** The data model, lattice and spatial indexes, the snapping algorithm, collision, the connection graph and structural checks, undo/redo and the collaboration path, rendering and performance, desktop versus touch interaction, play mode, build-instruction generation, and save/load. Dimensions and lattice rules come from [04](04-brick-geometry-spec.md); formats from [08](08-file-formats-and-interop.md).

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

Official parts: connectors derived by scanning LDraw primitive references (`stud.dat`, `stud4.dat`, `axlehole.dat`, `peghole.dat`, `clip*.dat`, `clh*.dat`, `phandle*.dat`). LDraw has no bar primitive, and some gear holes, clips, old hinges and towballs are drawn from plain geometry; these come from a CC BY 4.0 override table in the SNAP vocabulary, and contributors refine the rest the same way. Female studs are not primitives: they are synthesised from the part's geometry, as a stud-sized, stud-deep cavity with a wall, tube or pin that grips the stud. Clip and hinge axes come from a per-primitive calibration table ([Q04 spike](../spikes/q04-ldraw-connectivity.md)). Custom parts bring their own connectors from the part designer. The library is downloaded at first run (140 MiB) and packed into a versioned cache; a small CC0 starter set is bundled.

## 4. Spatial indexes

- **Lattice spatial hash** on 20 x 8 x 20 LDU brick cells using the Teschner hash: `(x * 73856093) xor (y * 19349663) xor (z * 83492791) mod n` with a prime table size, or a packed 64-bit key in an open-addressing map. Each instance is inserted into the cells its AABB covers: O(cells). Teschner's experiments show cell size dominates performance; we size cells to one brick.
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
| Snap query | O(connectors x neighbours), independent of model size |
| Lattice collision test | O(overlapping cells) bit operations |
| Attached check | near O(1) amortised (union-find) |
| Articulation update | O(V + E) of the affected component |
| Render | one draw per (part, colour, LOD, chunk) on WebGPU; one per material class on WebGL2 BatchedMesh |
| Undo | O(1) per command |
