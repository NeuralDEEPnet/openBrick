// The three.js viewer (decision D10): WebGPURenderer from 'three/webgpu', which
// falls back to WebGL2 by itself where WebGPU is missing, orbit controls (mouse
// and touch) all the way round so the underside shows too, a ground grid on the
// stud lattice, studio lighting and a soft contact shadow. It only displays
// meshes built by the core; the one transform it applies is the kernel-to-three.js
// frame rotation of packages/viewport and the lattice placement of
// src/placement.js.

import * as THREE from "three/webgpu";
import { OrbitControls } from "three/addons/controls/OrbitControls.js";
import { KERNEL_TO_THREE_EULER_X } from "@openbrick3d/viewport/frame.js";
import { brickOffset, framingDistance, gridCells, placedBounds } from "./placement.js";

/** Vertical field of view in degrees: a short telephoto, so the brick is not distorted. */
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
  // Orbit below the ground too: the underside (tubes, or pins under 1-wide bricks)
  // is where footprints differ most. The grid and the contact shadow hide while
  // the camera is below the ground (see the render loop).
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
  under.target = key.target; // both aim at the brick's centre
  scene.add(hemisphere, key, key.target, fill, rim, under);

  // Everything below `world` is in the kernel frame (mm, +Z up).
  const world = new THREE.Group();
  world.name = "kernel frame (mm, +Z up)";
  world.rotation.x = KERNEL_TO_THREE_EULER_X;
  scene.add(world);

  // Kernel meshes share vertices across sharp edges and carry no normals, so the
  // material shades flat from screen-space derivatives instead.
  const material = new THREE.MeshStandardNodeMaterial({ roughness: 0.38, metalness: 0, flatShading: true });
  const mesh = new THREE.Mesh(new THREE.BufferGeometry(), material);
  mesh.name = "brick";
  // The brick casts the contact shadow but does not receive shadows, which keeps
  // flat-shaded faces free of shadow acne.
  mesh.castShadow = true;
  mesh.visible = false;
  world.add(mesh);

  const ground = new THREE.Mesh(new THREE.PlaneGeometry(1, 1), new THREE.ShadowNodeMaterial({ opacity: 0.22 }));
  ground.rotation.x = -Math.PI / 2;
  ground.receiveShadow = true;
  scene.add(ground);
  let grid = null;
  let gridKey = "";

  let dirty = true;
  let frames = 0;
  /** How many bricks were shown, and which of them the last frame drew. */
  let shown = 0;
  let lastFrame = { triangles: 0, drawCalls: 0, drawn: 0, below: false };
  let standard = null;
  let current = null;

  function applyTheme() {
    scene.background = cssColour("--stage-bg", "#dfe6ee");
    material.color.copy(cssColour("--brick", "#c8323c"));
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
    // One cell per stud: lines on whole pitches from the origin, where the brick's
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
    controls.minDistance = bounds.radius * 0.8;
    controls.maxDistance = distance * 8;
    // Keep the lights and the shadow frustum around the brick.
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

  return {
    /** "webgpu" or "webgl2": the backend actually in use. */
    backend: renderer.backend.isWebGPUBackend ? "webgpu" : "webgl2",

    /** Use the standard summary from the core (pitch and undersize for placement). */
    setStandard(summary) {
      standard = summary;
      applyTheme();
    },

    /** Show a brick result from the core worker (src/protocol.js brick()). */
    showBrick(result) {
      if (!standard) throw new Error("setStandard() first");
      const geometry = new THREE.BufferGeometry();
      geometry.setAttribute("position", new THREE.BufferAttribute(result.positions, 3));
      geometry.setIndex(new THREE.BufferAttribute(result.indices, 1));
      geometry.computeBoundingSphere();
      mesh.geometry.dispose();
      mesh.geometry = geometry;
      const offset = brickOffset(result, standard);
      mesh.position.set(...offset);
      mesh.visible = true;
      shown++;
      const keepDirection = current !== null;
      current = { width: result.width, length: result.length };
      placeGrid(current);
      frame(placedBounds(result.bounds, offset), keepDirection);
      dirty = true;
    },

    applyTheme,

    /**
     * For the smoke test: frames drawn; the last frame's renderer.info counts, the
     * number of the brick it showed (`drawn`, equal to `shown` once the newest
     * brick is on screen) and whether it looked from below the ground (`below`);
     * the camera position.
     */
    stats() {
      return { frames, shown, ...lastFrame, camera: camera.position.toArray() };
    },
  };
}
