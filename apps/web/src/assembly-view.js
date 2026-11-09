// The three.js side of Build mode (docs/plan/07 section 9): the placed parts drawn as chunked
// InstancedMeshes, a translucent ghost of the part being placed, and an outline of the part under the
// pointer. It draws what the core says and turns pointer positions into rays in the model's frame; it never
// decides where a part goes (CLAUDE.md: the UI never computes geometry or connectivity).
//
// Chunks: a model of up to a few thousand parts is one chunk per part type (one draw call each, however many
// parts: the 14 built-in parts are at most 14 draw calls). A bigger model is cut over the lattice into cells
// of 32 x 32 x 32 studs (src/lod.js CHUNK_LDU), one chunk per (part type, cell). A chunk holds the matrices
// and colours of its parts in two typed arrays shared by its four meshes (one per render level), so an edit
// rewrites the arrays of the chunk it touches and uploads only that chunk; colours are per instance.
//
// Levels: each frame every chunk is tested against the camera frustum (a chunk out of view is not drawn)
// and drawn at the render level its studs' size on screen asks for (full studs, cheap studs and tubes, cheap
// studs on a solid body, the stud-less hull), with a margin so it does not flicker at a border, and borders
// that rise while the frame is over its triangle budget. The levels only change what is drawn: collision,
// picking and snapping are the core's, from boxes and connectors that do not depend on any render level.
//
// Frames: the model is in the LDraw frame (LDU, minus Y up). Everything below the `assembly` group is in
// that frame; the group turns half a revolution about X (a proper rotation: LDraw's minus Y is three.js's
// plus Y) and scales LDU to the millimetres the lighting of src/viewer.js is set up in.

import { colourHex, instanceMatrix } from "./build.js";
import { chunkCell, chunkKey, nextBias, pickLevel, studPixels, thresholds, triangleBudget, wantGrid } from "./lod.js";

/** Rotation of the LDraw frame into three.js's Y-up frame: half a turn about X. */
export const LDRAW_TO_THREE_EULER_X = Math.PI;

const GHOST = { valid: "#2fbf71", invalid: "#e5484d" };

/**
 * @param viewer  the viewer of src/viewer.js
 * @param options { catalog, lduMm }  the catalog JSON of the core
 */
export function createAssemblyView(viewer, { catalog: initialCatalog, lduMm }) {
  let catalog = initialCatalog;
  const { THREE, scene, camera, canvas, markDirty, setBuildActive, frameSphere } = viewer.internals;
  const root = new THREE.Group();
  root.name = "assembly (LDraw frame, LDU)";
  root.rotation.x = LDRAW_TO_THREE_EULER_X;
  root.scale.setScalar(lduMm);
  root.visible = false;
  scene.add(root);

  // Back faces are never drawn (FrontSide): a closed part shows its outside only, half the fragment work of a double-sided one.
  const material = new THREE.MeshStandardNodeMaterial({ roughness: 0.38, metalness: 0, flatShading: true, side: THREE.FrontSide });
  const ghostMaterial = new THREE.MeshStandardNodeMaterial({ roughness: 0.5, metalness: 0, flatShading: true, transparent: true, opacity: 0.6, depthWrite: false });
  const outlineMaterial = new THREE.LineBasicNodeMaterial({ color: "#ffd23f" });

  /** catalog part number -> { geometry, mesh, capacity } */
  const parts = new Map();
  const ghost = new THREE.Mesh(new THREE.BufferGeometry(), ghostMaterial);
  ghost.visible = false;
  ghost.matrixAutoUpdate = false;
  ghost.frustumCulled = false;
  ghost.renderOrder = 2;
  root.add(ghost);
  let ghostPart = -1;

  const outline = new THREE.Group();
  outline.visible = false;
  outline.matrixAutoUpdate = false;
  const box = new THREE.LineSegments(new THREE.EdgesGeometry(new THREE.BoxGeometry(1, 1, 1)), outlineMaterial);
  outline.add(box);
  root.add(outline);

  // The ground: the engine treats the whole Y = 0 plane as the table (a model needs no
  // baseplate), so Build mode draws it, a grid of studs on a plain plane, and a ghost is
  // never over an invisible floor. It sits a hair below the plane so a baseplate's
  // bottom does not flicker against it.
  const pitch = catalog.studPitchLdu;
  const groundStuds = 96;
  const groundSize = groundStuds * pitch;
  const groundMaterial = new THREE.MeshBasicNodeMaterial({ color: "#cdd6e0" });
  const plane = new THREE.Mesh(new THREE.PlaneGeometry(groundSize, groundSize), groundMaterial);
  plane.rotation.x = Math.PI / 2; // lies in XZ, facing up (LDraw minus Y)
  plane.position.y = 0.2;
  plane.renderOrder = -2;
  let grid = null;
  function paintGround() {
    const css = (name, fallback) => getComputedStyle(document.documentElement).getPropertyValue(name).trim() || fallback;
    groundMaterial.color.set(css("--ground", "#cdd6e0"));
    if (grid) {
      root.remove(grid);
      grid.geometry.dispose();
      grid.material.dispose();
    }
    grid = new THREE.GridHelper(groundSize, groundStuds, css("--grid-major", "#7d8a99"), css("--grid-minor", "#b4bfcb"));
    grid.position.y = 0.1;
    root.add(grid);
  }
  root.add(plane);
  paintGround();

  const colour = new THREE.Color();
  /** palette code -> linear [r, g, b] as three.js stores it in an instance colour. */
  const colourCache = new Map();
  function rgbOf(code) {
    let c = colourCache.get(code);
    if (!c) {
      colour.set(colourHex(catalog.palette, code));
      c = [colour.r, colour.g, colour.b];
      colourCache.set(code, c);
    }
    return c;
  }

  /** The chunks: key "part|cell" -> chunk. */
  const chunks = new Map();
  /** id -> the key of the chunk the part is in. */
  const where = new Map();
  const dirty = new Set();
  let table = null;
  /** Whether the model is cut over the lattice (src/lod.js wantGrid, `chunked`) and whether it is small enough for full detail everywhere. */
  let chunked = false;
  let small = true;
  let bias = 1;
  let budget = triangleBudget({ phone: false });
  /** The last frame's counts. */
  /** The counts of the last frame, written in place each frame (nothing is allocated per frame). */
  const frameStats = { drawCalls: 0, triangles: 0, parts: 0, visibleChunks: 0, lod: [0, 0, 0, 0] };
  let stats = frameStats;
  let lastScreen = { heightPx: 800, fov: 35 };

  function geometryOf(m) {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(m.positions, 3));
    geometry.setIndex(new THREE.BufferAttribute(m.indices, 1));
    geometry.computeBoundingSphere();
    return geometry;
  }

  /** Half the longest diagonal of a part's pick box from its origin: a radius that holds it in any rotation. */
  function reachOf(index) {
    const b = catalog.parts[index].pickBox;
    return Math.hypot(...[0, 1, 2].map((k) => Math.max(Math.abs(b.min[k]), Math.abs(b.max[k]))));
  }

  function disposeChunk(chunk) {
    for (const mesh of chunk.meshes) {
      if (!mesh) continue;
      root.remove(mesh);
      mesh.dispose?.();
    }
    chunk.meshes = [null, null, null, null];
    // No mesh is drawn for this chunk now: the next selectLevels makes the one it wants (a stale `shown` would skip that).
    chunk.shown = -1;
  }

  function dropAllChunks() {
    for (const chunk of chunks.values()) disposeChunk(chunk);
    chunks.clear();
    where.clear();
    dirty.clear();
  }

  /** Use another catalog (a part pack was added or removed); part numbers of the parts already here are unchanged. */
  function setCatalog(next) {
    catalog = next;
    colourCache.clear();
    // A part number may name another part now (the pack was replaced): forget what was uploaded.
    for (const [index, entry] of [...parts]) {
      if (!next.parts[index] || entry.id !== next.parts[index].id) release([index]);
    }
  }

  /** Whether the meshes of every render level of catalog part `index` are uploaded. */
  function has(index) {
    const e = parts.get(index);
    return !!e && e.levels.every(Boolean);
  }

  /**
   * Upload render meshes ([{ index, lod, positions, indices, triangles }], `lod` 0 to 3, defaulting to 0): a part
   * has four levels. A part with levels 0 and 3 only (a part of an LDraw pack has one mesh and its hull) uses
   * level 0 for 1 and 2. A level that is already here is replaced, and the instances using it are drawn
   * again at the next frame.
   */
  function addMeshes(meshes) {
    const replaced = new Set(meshes.map((m) => m.index));
    for (const m of meshes) {
      const lod = m.lod ?? 0;
      let entry = parts.get(m.index);
      if (!entry) {
        entry = { levels: [null, null, null, null], tris: [0, 0, 0, 0], id: catalog.parts[m.index]?.id, reach: reachOf(m.index) };
        parts.set(m.index, entry);
      }
      if (entry.levels[lod]) entry.levels[lod].dispose();
      entry.levels[lod] = geometryOf(m);
      entry.tris[lod] = m.triangles;
      if (ghostPart === m.index) {
        ghost.visible = false;
        ghostPart = -1;
      }
    }
    for (const entry of parts.values()) {
      if (entry.levels[0] && entry.levels[3]) {
        // A part with one mesh of its own: its middle levels are that mesh.
        entry.levels[1] ??= entry.levels[0];
        entry.levels[2] ??= entry.levels[0];
        entry.tris[1] ||= entry.tris[0];
        entry.tris[2] ||= entry.tris[0];
      }
    }
    // Chunks of these parts hold meshes made from the old geometries (or none yet): make them again.
    for (const chunk of chunks.values()) {
      if (!replaced.has(chunk.part)) continue;
      disposeChunk(chunk);
      chunk.matrices = null; // rebuilt from the table, with meshes of the new geometries
      dirty.add(chunk.key);
    }
    if (table && dirty.size > 0) flush();
    else markDirty();
  }

  /** Let go of the uploaded meshes of these part numbers (GPU memory); not the ghost's. */
  function release(indices) {
    for (const index of indices) {
      const entry = parts.get(index);
      if (!entry) continue;
      if (ghostPart === index) {
        ghost.visible = false;
        ghost.geometry = new THREE.BufferGeometry();
        ghostPart = -1;
      }
      for (const [key, chunk] of [...chunks]) {
        if (chunk.part === index) {
          disposeChunk(chunk);
          chunks.delete(key);
        }
      }
      for (const g of new Set(entry.levels)) g?.dispose();
      parts.delete(index);
    }
    markDirty();
  }

  /** The part numbers with an uploaded mesh, oldest upload first. */
  function resident() {
    return [...parts.keys()];
  }

  // ------------------------------------------------------------- chunks

  function keyOf(slot) {
    const part = table.partAt(slot);
    return chunkKey(part, chunkCell([table.posAt(slot, 0), table.posAt(slot, 1), table.posAt(slot, 2)], chunked));
  }

  function chunkAt(key, slot) {
    let chunk = chunks.get(key);
    if (!chunk) {
      chunk = {
        key,
        part: table.partAt(slot),
        ids: new Set(),
        capacity: 0,
        count: 0,
        matrices: null,
        colours: null,
        matrixAttr: null,
        colourAttr: null,
        meshes: [null, null, null, null],
        centre: [0, 0, 0],
        radius: 0,
        level: -1,
        shown: -1,
      };
      chunks.set(key, chunk);
    }
    return chunk;
  }

  /**
   * Show the model in `next` (a src/model-table.js ModelTable): every chunk is made again. Used when a model is
   * opened or replaced; edits go through [`update`].
   */
  function setModel(next) {
    table = next;
    dropAllChunks();
    chunked = wantGrid(table.size, chunked);
    table.forEachSlot((slot) => {
      const key = keyOf(slot);
      chunkAt(key, slot).ids.add(table.idAt(slot));
      where.set(table.idAt(slot), key);
      dirty.add(key);
    });
    flush();
  }

  /** The table changed for these ids (after `table.applyDelta`): move each part to its chunk and mark the chunks to redraw. */
  function update(ids) {
    if (!table) return;
    if (wantGrid(table.size, chunked) !== chunked) return setModel(table);
    for (const id of ids) {
      const old = where.get(id);
      const slot = table.slotOf(id);
      if (slot === undefined) {
        if (old !== undefined) {
          chunks.get(old)?.ids.delete(id);
          dirty.add(old);
          where.delete(id);
        }
        continue;
      }
      const key = keyOf(slot);
      if (old !== undefined && old !== key) {
        chunks.get(old)?.ids.delete(id);
        dirty.add(old);
      }
      chunkAt(key, slot).ids.add(id);
      where.set(id, key);
      dirty.add(key);
    }
    flush();
  }

  /** Write the chunks that changed: their typed arrays, bounds and meshes. */
  function flush() {
    for (const key of dirty) {
      const chunk = chunks.get(key);
      if (!chunk) continue;
      if (chunk.ids.size === 0) {
        disposeChunk(chunk);
        chunks.delete(key);
        continue;
      }
      rebuild(chunk);
    }
    dirty.clear();
    selectLevels();
    markDirty();
  }

  function rebuild(chunk) {
    const entry = parts.get(chunk.part);
    if (!entry) return; // its meshes are still on the way; they redraw the chunk when they arrive
    const n = chunk.ids.size;
    if (chunk.capacity < n || !chunk.matrices) {
      // New arrays (and so new meshes): growing by a quarter again, so a run of placements does not remake them each time.
      disposeChunk(chunk);
      chunk.capacity = Math.max(16, Math.ceil(n * 1.25));
      chunk.matrices = new Float32Array(chunk.capacity * 16);
      chunk.colours = new Float32Array(chunk.capacity * 3);
      chunk.matrixAttr = new THREE.InstancedBufferAttribute(chunk.matrices, 16);
      chunk.matrixAttr.setUsage(THREE.DynamicDrawUsage);
      chunk.colourAttr = new THREE.InstancedBufferAttribute(chunk.colours, 3);
      chunk.colourAttr.setUsage(THREE.DynamicDrawUsage);
    }
    const reach = entry.reach;
    const lo = [Infinity, Infinity, Infinity];
    const hi = [-Infinity, -Infinity, -Infinity];
    let i = 0;
    const rotations = catalog.rotations;
    for (const id of chunk.ids) {
      const slot = table.slotOf(id);
      const m = rotations[table.rotAt(slot)];
      const o = i * 16;
      const a = chunk.matrices;
      a[o] = m[0][0];
      a[o + 1] = m[1][0];
      a[o + 2] = m[2][0];
      a[o + 3] = 0;
      a[o + 4] = m[0][1];
      a[o + 5] = m[1][1];
      a[o + 6] = m[2][1];
      a[o + 7] = 0;
      a[o + 8] = m[0][2];
      a[o + 9] = m[1][2];
      a[o + 10] = m[2][2];
      a[o + 11] = 0;
      for (let k = 0; k < 3; k++) {
        const p = table.posAt(slot, k);
        a[o + 12 + k] = p;
        lo[k] = Math.min(lo[k], p - reach);
        hi[k] = Math.max(hi[k], p + reach);
      }
      a[o + 15] = 1;
      chunk.colours.set(rgbOf(table.colourAt(slot)), i * 3);
      i++;
    }
    chunk.count = n;
    chunk.centre = [(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, (lo[2] + hi[2]) / 2];
    chunk.radius = Math.hypot(hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]) / 2;
    chunk.matrixAttr.needsUpdate = true;
    chunk.colourAttr.needsUpdate = true;
    for (const mesh of chunk.meshes) if (mesh) mesh.count = n;
  }

  function meshOf(chunk, level) {
    let mesh = chunk.meshes[level];
    if (!mesh) {
      const entry = parts.get(chunk.part);
      mesh = new THREE.InstancedMesh(entry.levels[level], material, chunk.capacity);
      // The arrays are the chunk's, shared by the meshes of its four levels (one upload, whichever is drawn).
      mesh.instanceMatrix = chunk.matrixAttr;
      mesh.instanceColor = chunk.colourAttr;
      mesh.count = chunk.count;
      mesh.frustumCulled = false; // the view tests the chunk itself, see selectLevels
      mesh.name = `${catalog.parts[chunk.part].id} ${chunk.key} level ${level}`;
      mesh.visible = false;
      root.add(mesh);
      chunk.meshes[level] = mesh;
    }
    return mesh;
  }

  // ------------------------------------------------------------- per frame

  const frustum = new THREE.Frustum();
  const projScreen = new THREE.Matrix4();
  const sphere = new THREE.Sphere();
  const centreWorld = new THREE.Vector3();
  const cameraWorld = new THREE.Vector3();

  /**
   * Decide, for every chunk, whether it is in view and at which render level it is drawn, and count what the
   * frame will draw. Called when the model changes and before every frame the viewer draws.
   */
  function selectLevels() {
    root.updateMatrixWorld(true);
    camera.updateMatrixWorld(true);
    projScreen.multiplyMatrices(camera.projectionMatrix, camera.matrixWorldInverse);
    frustum.setFromProjectionMatrix(projScreen);
    cameraWorld.setFromMatrixPosition(camera.matrixWorld);
    const stud = lduMm * catalog.studPitchLdu;
    const limits = thresholds({ small, bias });
    const view = frameStats;
    view.drawCalls = view.triangles = view.parts = view.visibleChunks = 0;
    view.lod.fill(0);
    for (const chunk of chunks.values()) {
      const entry = parts.get(chunk.part);
      if (!entry || !chunk.matrices) continue;
      centreWorld.set(chunk.centre[0], chunk.centre[1], chunk.centre[2]).applyMatrix4(root.matrixWorld);
      sphere.center.copy(centreWorld);
      sphere.radius = chunk.radius * lduMm;
      const visible = frustum.intersectsSphere(sphere);
      let level = chunk.level < 0 ? 0 : chunk.level;
      if (visible) {
        const distance = Math.max(centreWorld.distanceTo(cameraWorld) - sphere.radius, camera.near || 1);
        level = pickLevel(studPixels(stud, distance, lastScreen.fov, lastScreen.heightPx), limits, chunk.level);
        // A part with a single mesh of its own (a pack part) has no cheaper level than its own.
        while (level > 0 && !entry.levels[level]) level--;
      }
      chunk.level = level;
      const show = visible ? level : -1;
      if (show !== chunk.shown) {
        if (chunk.shown >= 0 && chunk.meshes[chunk.shown]) chunk.meshes[chunk.shown].visible = false;
        if (show >= 0) meshOf(chunk, show).visible = true;
        chunk.shown = show;
      }
      if (visible) {
        // Counted from the mesh that is drawn (its instance count), never from the table.
        const drawn = chunk.meshes[level];
        if (drawn?.visible && drawn.count > 0) {
          view.drawCalls++;
          view.visibleChunks++;
          view.parts += drawn.count;
          view.lod[level]++;
          view.triangles += drawn.count * entry.tris[level];
        }
      }
    }
    stats = view;
  }

  /** Called by the viewer before it draws a frame: levels for the camera as it is, and the next frame's bias. */
  function beforeFrame() {
    const size = viewer.internals.stageSize?.();
    if (size) lastScreen = { heightPx: size.heightPx, fov: camera.fov };
    selectLevels();
    // Over the triangle budget: the borders rise for the next frame (src/lod.js nextBias); with a dead band.
    const next = nextBias(bias, stats.triangles, budget);
    if (next !== bias) {
      bias = next;
      viewer.internals.markDirty();
    }
  }
  viewer.internals.onBeforeRender?.(beforeFrame);

  /**
   * Show the ghost of catalog part `part` at the pose the core snapped to, or
   * hide it with null. `state` is "valid" or "invalid".
   */
  function setGhost(g) {
    if (!g) {
      if (ghost.visible) {
        ghost.visible = false;
        markDirty();
      }
      return;
    }
    const entry = parts.get(g.part);
    const geometry = entry?.levels[0] ?? entry?.levels[1];
    if (!geometry) return; // its mesh is still on the way
    if (g.part !== ghostPart) {
      ghost.geometry = geometry;
      ghostPart = g.part;
    }
    ghost.matrix.fromArray(instanceMatrix(catalog.rotations, g.rot, g.pos));
    ghost.matrixWorldNeedsUpdate = true;
    ghostMaterial.color.set(GHOST[g.state]);
    ghost.visible = true;
    markDirty();
  }

  /** Outline the placed part `inst` ({ part, pos, rot }), or none with null. */
  function setOutline(inst) {
    if (!inst) {
      if (outline.visible) {
        outline.visible = false;
        markDirty();
      }
      return;
    }
    const b = catalog.parts[inst.part].pickBox;
    outline.matrix.fromArray(instanceMatrix(catalog.rotations, inst.rot, inst.pos));
    outline.matrixWorldNeedsUpdate = true;
    box.scale.set(b.max[0] - b.min[0], b.max[1] - b.min[1], b.max[2] - b.min[2]);
    box.position.set((b.min[0] + b.max[0]) / 2, (b.min[1] + b.max[1]) / 2, (b.min[2] + b.max[2]) / 2);
    outline.visible = true;
    markDirty();
  }

  const near = new THREE.Vector3();
  const far = new THREE.Vector3();

  /**
   * The pointer ray at client coordinates, in the model's frame (LDU): { origin,
   * dir }. This is where the pointer is, not where a part goes.
   */
  function pointerRay(clientX, clientY) {
    const r = canvas.getBoundingClientRect();
    const x = ((clientX - r.left) / r.width) * 2 - 1;
    const y = -((clientY - r.top) / r.height) * 2 + 1;
    root.updateMatrixWorld(true);
    camera.updateMatrixWorld(true);
    near.set(x, y, -1).unproject(camera);
    far.set(x, y, 1).unproject(camera);
    root.worldToLocal(near);
    root.worldToLocal(far);
    return { origin: near.toArray(), dir: far.sub(near).toArray() };
  }

  /** Client coordinates of a point in the model's frame (LDU), for tests and tours. */
  function project(pos) {
    root.updateMatrixWorld(true);
    camera.updateMatrixWorld(true);
    const v = new THREE.Vector3(...pos);
    root.localToWorld(v);
    v.project(camera);
    const r = canvas.getBoundingClientRect();
    return { x: r.left + ((v.x + 1) / 2) * r.width, y: r.top + ((1 - v.y) / 2) * r.height };
  }

  /** Frame the baseplate and what stands on it: a sphere in LDU of the model frame. */
  function frameLdu(center, radius, keepDirection = false) {
    const c = new THREE.Vector3(...center);
    root.updateMatrixWorld(true);
    root.localToWorld(c);
    frameSphere(c.toArray(), radius * lduMm, keepDirection);
  }

  function show(on) {
    if (on) paintGround();
    root.visible = on;
    setBuildActive(on);
  }

  return {
    setCatalog,
    addMeshes,
    release,
    has,
    resident,
    setModel,
    update,
    /** Whether full detail is used everywhere (a small model); and the triangle budget of a frame. */
    setPolicy(policy) {
      if (policy.small !== undefined && policy.small !== small) {
        small = policy.small;
        bias = 1;
      }
      if (policy.trianglesBudget !== undefined) budget = policy.trianglesBudget;
      selectLevels();
      markDirty();
    },
    setGhost,
    setOutline,
    pointerRay,
    project,
    frameLdu,
    show,
    /** Instances in the meshes that are drawn, counted from the scene itself (for tests and the smoke). */
    drawnInstances() {
      let n = 0;
      root.traverse((o) => {
        if (o.isInstancedMesh && o.visible) n += o.count;
      });
      return n;
    },
    /** What the last frame draws: triangles, parts, draw calls (chunks in view), and the render levels of those chunks. */
    counts: () => ({ triangles: stats.triangles, parts: stats.parts, drawCalls: stats.drawCalls, chunks: chunks.size, visibleChunks: stats.visibleChunks, lod: [...stats.lod], bias, chunked, small }),
  };
}
