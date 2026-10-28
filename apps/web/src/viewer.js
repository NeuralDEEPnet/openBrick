// The three.js viewer (decision D10): WebGPURenderer from 'three/webgpu', which
// falls back to WebGL2 by itself where WebGPU is missing, orbit controls (mouse
// and touch) all the way round so the underside shows too, a ground grid on the
// stud lattice, studio lighting and a soft contact shadow. It only displays
// meshes built by the core: one per colour role, plus the red overlay of artwork
// that does not print as drawn. The one transform it applies is the
// kernel-to-three.js frame rotation of packages/viewport and the placement of
// src/placement.js.

import * as THREE from "three/webgpu";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { KERNEL_TO_THREE_EULER_X } from "@openbrick3d/viewport/frame.js";
import { brickOffset, centredOffset, framingDistance, gridCells, placedBounds } from "./placement.js";

/** Vertical field of view in degrees: a short telephoto, so the part is not distorted. */
const FOV_DEG = 35;
/** Direction from the target to the camera for the first view: front right, above. */
const FIRST_VIEW = new THREE.Vector3(1, 0.9, 1.5).normalize();
/** Device pixel ratio cap; above 2 the cost grows and phones gain nothing visible. */
const MAX_PIXEL_RATIO = 2;

/** A CSS custom property of the page as a three.js colour (in the sRGB working space). */
function cssColour(name, fallback) {
  const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
  return new THREE.Color(value || fallback);
}

/**
 * Create the viewer inside `container` (which it fills). `backend` is "auto",
 * "webgpu" or "webgl2"; "webgl2" forces the WebGL2 backend.
 */
export async function createViewer(container, { backend = "auto", onDeviceLost } = {}) {
  const renderer = new THREE.WebGPURenderer({ antialias: true, forceWebGL: backend === "webgl2" });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, MAX_PIXEL_RATIO));
  renderer.shadowMap.enabled = true;
  renderer.domElement.setAttribute("aria-hidden", "true");
  container.append(renderer.domElement);
  await renderer.init();
  renderer.onDeviceLost = (info) => onDeviceLost?.(new Error(`${info.api} device lost: ${info.message}`));

  const scene = new THREE.Scene();
  const camera = new THREE.PerspectiveCamera(FOV_DEG, 1, 1, 1000);
  const controls = new OrbitControls(camera, renderer.domElement);
  controls.enableDamping = true;
  // Orbit below the ground too: the underside (tubes) is where parts differ
  // most. The grid and the contact shadow hide while the camera is below the
  // ground (see the render loop).
  controls.minPolarAngle = 0;
  controls.maxPolarAngle = Math.PI;

  // Studio lighting: a sky/ground fill, a key light that casts the contact shadow,
  // a cool fill from the other side, a rim light from behind, and a soft light
  // from below so the underside reads (every other direct light comes from above,
  // so faces pointing down would get only the dim ground colour).
  const hemisphere = new THREE.HemisphereLight(0xffffff, 0x3a4250, 1.1);
  const key = new THREE.DirectionalLight(0xffffff, 2.4);
  key.castShadow = true;
  key.shadow.mapSize.set(1024, 1024);
  const fill = new THREE.DirectionalLight(0xdbe8ff, 0.6);
  const rim = new THREE.DirectionalLight(0xffffff, 0.9);
  const under = new THREE.DirectionalLight(0xfff4e8, 0.9);
  under.target = key.target; // both aim at the part's centre
  scene.add(hemisphere, key, key.target, fill, rim, under);

  // Everything below `world` is in the kernel frame (mm, +Z up).
  const world = new THREE.Group();
  world.name = "kernel frame (mm, +Z up)";
  world.rotation.x = KERNEL_TO_THREE_EULER_X;
  scene.add(world);
  const part = new THREE.Group();
  part.name = "part";
  world.add(part);

  /** role -> material; kernel meshes share vertices across sharp edges and carry
   * no normals, so the materials shade flat from screen-space derivatives. */
  const materials = new Map();
  /** role -> colour chosen by the page (overrides the core's role colour). */
  const chosen = new Map();
  function material(role, coreColour) {
    let m = materials.get(role);
    if (!m) {
      if (role === "removed") {
        // The overlay reads as flat paint on the face: unlit, and pulled towards
        // the camera in the depth test so it never flickers against the face.
        m = new THREE.MeshBasicNodeMaterial({ polygonOffset: true, polygonOffsetFactor: -2, polygonOffsetUnits: -2 });
      } else {
        m = new THREE.MeshStandardNodeMaterial({ roughness: 0.38, metalness: 0, flatShading: true });
      }
      materials.set(role, m);
    }
    m.color.set(chosen.get(role) ?? coreColour);
    return m;
  }

  const ground = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), new THREE.ShadowNodeMaterial({ opacity: 0.22 }));
  ground.rotation.x = -Math.PI / 2;
  ground.receiveShadow = true;
  scene.add(ground);
  let grid = null;
  let gridKey = "";

  let dirty = true;
  let frames = 0;
  /** How many parts were shown, and which of them the last frame drew. */
  let shown = 0;
  let lastFrame = { triangles: 0, drawCalls: 0, drawn: 0, below: false };
  let standard = null;
  /** The footprint the grid was laid out for, in studs. */
  let current = null;
  /** What was shown last, for the smoke test: roles, triangles, overlay. */
  let showing = null;

  function applyTheme() {
    scene.background = cssColour("--stage-bg", "#dfe6ee");
    gridKey = ""; // rebuild the grid in the new colours
    if (current) placeGrid(current);
    dirty = true;
  }

  function placeGrid(size) {
    const cells = gridCells(size);
    const keyNow = `${cells}`;
    if (keyNow === gridKey) return;
    gridKey = keyNow;
    if (grid) {
      scene.remove(grid);
      grid.geometry.dispose();
      grid.material.dispose();
    }
    // One cell per stud: lines on whole pitches from the origin, where the part's
    // lattice cell corners sit (src/placement.js).
    const extent = cells * standard.pitchMm;
    grid = new THREE.GridHelper(extent, cells, cssColour("--grid-major", "#8c99a8"), cssColour("--grid-minor", "#b9c3ce"));
    grid.name = "stud lattice";
    // Lift the lines a hair above the shadow plane so they do not flicker with it.
    grid.position.y = standard.pitchMm * 1e-3;
    scene.add(grid);
    ground.scale.set(extent, extent, 1);
  }

  function frame(bounds, keepDirection) {
    const direction = keepDirection
      ? camera.position.clone().sub(controls.target).normalize()
      : FIRST_VIEW.clone();
    const center = new THREE.Vector3(...bounds.center);
    const distance = framingDistance(bounds.radius, FOV_DEG, camera.aspect || 1);
    controls.target.copy(center);
    camera.position.copy(center).addScaledVector(direction, distance);
    camera.near = distance / 100;
    camera.far = distance * 50;
    camera.updateProjectionMatrix();
    controls.minDistance = bounds.radius * 0.5;
    controls.maxDistance = distance * 8;
    // Keep the lights and the shadow frustum around the part.
    const r = bounds.radius;
    key.target.position.copy(center);
    key.position.copy(center).add(new THREE.Vector3(0.55, 1, 0.4).normalize().multiplyScalar(4 * r));
    fill.position.copy(center).add(new THREE.Vector3(-1, 0.45, -0.2).multiplyScalar(4 * r));
    rim.position.copy(center).add(new THREE.Vector3(-0.2, 0.7, -1).multiplyScalar(4 * r));
    under.position.copy(center).add(new THREE.Vector3(0.3, -1, 0.5).multiplyScalar(4 * r));
    const shadowCamera = key.shadow.camera;
    shadowCamera.left = shadowCamera.bottom = -1.5 * r;
    shadowCamera.right = shadowCamera.top = 1.5 * r;
    shadowCamera.near = r;
    shadowCamera.far = 8 * r;
    shadowCamera.updateProjectionMatrix();
    controls.update();
  }

  function resize() {
    const width = Math.max(1, container.clientWidth);
    const height = Math.max(1, container.clientHeight);
    renderer.setSize(width, height);
    camera.aspect = width / height;
    camera.updateProjectionMatrix();
    dirty = true;
  }

  new ResizeObserver(resize).observe(container);
  resize();
  controls.addEventListener("change", () => {
    dirty = true;
  });

  // Render only when something changed: the loop itself is cheap, a frame is not
  // (phones). Damping keeps update() returning true until the camera settles.
  renderer.setAnimationLoop(() => {
    const moved = controls.update();
    if (!moved && !dirty) return;
    dirty = false;
    // Seen from below, the grid would cut across the open underside and the
    // shadow would sit in front of it; both belong to the floor, so they hide.
    const below = camera.position.y < 0;
    ground.visible = !below;
    if (grid) grid.visible = !below;
    renderer.render(scene, camera);
    frames++;
    // info is reset at the start of each animation frame, so this is the whole
    // frame: shadow pass, scene and the output pass.
    lastFrame = { triangles: renderer.info.render.triangles, drawCalls: renderer.info.render.drawCalls, drawn: shown, below };
  });

  function clearPart() {
    for (const child of [...part.children]) {
      part.remove(child);
      child.geometry.dispose();
    }
  }

  function geometryOf(m) {
    const geometry = new THREE.BufferGeometry();
    geometry.setAttribute("position", new THREE.BufferAttribute(m.positions, 3));
    geometry.setIndex(new THREE.BufferAttribute(m.indices, 1));
    geometry.computeBoundingSphere();
    return geometry;
  }

  return {
    /** "webgpu" or "webgl2": the backend actually in use. */
    backend: renderer.backend.isWebGPUBackend ? "webgpu" : "webgl2",

    /** Use the standard summary from the core (pitch and undersize for placement). */
    setStandard(summary) {
      standard = summary;
      applyTheme();
    },

    /**
     * Show a part from the core worker: `meshes` (one per colour role) and an
     * optional `overlay` (role "removed"), all in the kernel frame. With a
     * `footprint` [x, y] in studs the part sits on the stud lattice; without one
     * it is centred on the grid (the coupon). `keepView` keeps the camera
     * direction (the same part changed); otherwise the first view is restored.
     */
    showPart({ meshes, overlay = null, footprint = null }, { keepView = true } = {}) {
      if (!standard) throw new Error("setStandard() first");
      clearPart();
      const lo = [Infinity, Infinity, Infinity];
      const hi = [-Infinity, -Infinity, -Infinity];
      for (const m of meshes) {
        for (let k = 0; k < 3; k++) {
          lo[k] = Math.min(lo[k], m.bounds[k]);
          hi[k] = Math.max(hi[k], m.bounds[k + 3]);
        }
      }
      const bounds = [...lo, ...hi];
      let size;
      let offset;
      if (footprint) {
        size = { width: footprint[0], length: footprint[1] };
        offset = brickOffset(size, standard);
      } else {
        offset = centredOffset(bounds);
        size = { width: Math.ceil((hi[0] - lo[0]) / standard.pitchMm), length: Math.ceil((hi[1] - lo[1]) / standard.pitchMm) };
      }
      part.position.set(...offset);
      for (const m of meshes) {
        const mesh = new THREE.Mesh(geometryOf(m), material(m.role, m.color));
        mesh.name = m.role;
        // Parts cast the contact shadow but do not receive shadows, which keeps
        // flat-shaded faces free of shadow acne.
        mesh.castShadow = true;
        part.add(mesh);
      }
      if (overlay) {
        const mesh = new THREE.Mesh(geometryOf(overlay), material("removed", overlay.color));
        mesh.name = "removed";
        mesh.renderOrder = 1;
        part.add(mesh);
      }
      shown++;
      const keepDirection = keepView && current !== null;
      current = size;
      placeGrid(current);
      frame(placedBounds(bounds, offset), keepDirection);
      showing = {
        roles: meshes.map((m) => m.role),
        triangles: meshes.reduce((n, m) => n + m.triangles, 0),
        overlayTriangles: overlay?.triangles ?? 0,
      };
      dirty = true;
    },

    /**
     * Show no part (the build failed, so nothing on screen describes the
     * controls). The grid and the camera stay; the page says why in text.
     */
    clear() {
      clearPart();
      shown++;
      showing = { roles: [], triangles: 0, overlayTriangles: 0 };
      dirty = true;
    },

    /** Recolour roles without a new part: { body: "#rrggbb", accent: ... }. */
    setColors(colors) {
      for (const [role, colour] of Object.entries(colors)) {
        chosen.set(role, colour);
        materials.get(role)?.color.set(colour);
      }
      dirty = true;
    },

    applyTheme,

    /**
     * For the smoke test: frames drawn; the last frame's renderer.info counts, the
     * number of the part it showed (`drawn`, equal to `shown` once the newest
     * part is on screen) and whether it looked from below the ground (`below`);
     * the camera position; what is shown.
     */
    stats() {
      return { frames, shown, ...lastFrame, camera: camera.position.toArray(), showing };
    },
  };
}
