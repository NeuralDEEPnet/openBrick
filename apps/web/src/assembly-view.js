// The three.js side of Build mode (docs/plan/07 section 9): one InstancedMesh per
// catalog part with the colour per instance (14 draw calls at most, however many
// parts), a translucent ghost of the part being placed, and an outline of the
// part under the pointer. It draws what the core says and turns pointer
// positions into rays in the model's frame; it never decides where a part goes
// (CLAUDE.md: the UI never computes geometry or connectivity).
//
// Frames: the model is in the LDraw frame (LDU, minus Y up). Everything below the
// `assembly` group is in that frame; the group turns half a revolution about X
// (a proper rotation: LDraw's minus Y is three.js's plus Y) and scales LDU to the
// millimetres the lighting of src/viewer.js is set up in.

import { colourHex, instanceMatrix } from "./build.js";

/** Rotation of the LDraw frame into three.js's Y-up frame: half a turn about X. */
export const LDRAW_TO_THREE_EULER_X = Math.PI;

const GHOST = { valid: "#2fbf71", invalid: "#e5484d" };

/**
 * @param viewer  the viewer of src/viewer.js
 * @param options { catalog, lduMm }  the catalog JSON of the core
 */
export function createAssemblyView(viewer, { catalog, lduMm }) {
  const { THREE, scene, camera, canvas, markDirty, setBuildActive, frameSphere } = viewer.internals;
  const root = new THREE.Group();
  root.name = "assembly (LDraw frame, LDU)";
  root.rotation.x = LDRAW_TO_THREE_EULER_X;
  root.scale.setScalar(lduMm);
  root.visible = false;
  scene.add(root);

  const material = new THREE.MeshStandardNodeMaterial({ roughness: 0.38, metalness: 0, flatShading: true });
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
  const matrix = new THREE.Matrix4();
  let triangles = 0;
  let drawn = 0;

  function geometryOf(m) {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(m.positions, 3));
    geometry.setIndex(new THREE.BufferAttribute(m.indices, 1));
    geometry.computeBoundingSphere();
    return geometry;
  }

  function dropMesh(entry) {
    if (entry.mesh) {
      root.remove(entry.mesh);
      entry.mesh.dispose?.();
      entry.mesh = null;
    }
  }

  /** The render meshes of the catalog parts: [{ index, positions, indices, triangles }]. */
  function setMeshes(meshes) {
    // The ghost still holds one of the geometries about to be disposed: let go of it first.
    ghost.visible = false;
    ghost.geometry = new THREE.BufferGeometry();
    ghostPart = -1;
    for (const entry of parts.values()) {
      dropMesh(entry);
      entry.geometry.dispose();
    }
    parts.clear();
    for (const m of meshes) parts.set(m.index, { geometry: geometryOf(m), triangles: m.triangles, mesh: null, capacity: 0 });
    markDirty();
  }

  function ensureMesh(index, count) {
    const entry = parts.get(index);
    if (!entry) throw new RangeError(`no mesh for catalog part ${index}`);
    if (!entry.mesh || entry.capacity < count) {
      dropMesh(entry);
      entry.capacity = Math.max(64, count * 2);
      const mesh = new THREE.InstancedMesh(entry.geometry, material, entry.capacity);
      mesh.name = catalog.parts[index].id;
      mesh.frustumCulled = false;
      mesh.instanceMatrix.setUsage(THREE.DynamicDrawUsage);
      mesh.count = 0;
      root.add(mesh);
      entry.mesh = mesh;
    }
    return entry.mesh;
  }

  /** Show the placed parts: [{ id, part, pos, rot, colour }] grouped by part number. */
  function setInstances(groups) {
    triangles = 0;
    drawn = 0;
    for (const [index, entry] of parts) {
      const list = groups.get(index) ?? [];
      if (list.length === 0) {
        if (entry.mesh) entry.mesh.count = 0;
        continue;
      }
      const mesh = ensureMesh(index, list.length);
      list.forEach((inst, i) => {
        matrix.fromArray(instanceMatrix(catalog.rotations, inst.rot, inst.pos));
        mesh.setMatrixAt(i, matrix);
        mesh.setColorAt(i, colour.set(colourHex(catalog.palette, inst.colour)));
      });
      mesh.count = list.length;
      mesh.instanceMatrix.needsUpdate = true;
      if (mesh.instanceColor) mesh.instanceColor.needsUpdate = true;
      triangles += entry.triangles * list.length;
      drawn += list.length;
    }
    markDirty();
  }

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
    if (g.part !== ghostPart) {
      ghost.geometry = parts.get(g.part).geometry;
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
    setMeshes,
    setInstances,
    setGhost,
    setOutline,
    pointerRay,
    project,
    frameLdu,
    show,
    /** Triangles and parts drawn by the last setInstances. */
    counts: () => ({ triangles, parts: drawn, drawCalls: [...parts.values()].filter((p) => p.mesh && p.mesh.count > 0).length }),
  };
}
