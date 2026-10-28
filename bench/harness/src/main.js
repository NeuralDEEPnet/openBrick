// Device render harness (docs/plan/07 section 9, decision D10 in docs/plan/03,
// backlog item 3 in docs/plan/14). Renders N instanced 2x4 bricks built by the
// geometry kernel at one LOD with the three.js WebGPURenderer and reports
// frame-time statistics as one CSV row. Column meanings: bench/devices/README.md.

import * as THREE from "three/webgpu";
import { COLUMN_NAMES, SCHEMA_VERSION, csvHeader, csvRow } from "./csv.js";
import { KERNEL_TO_THREE_EULER_X } from "./frame.js";
import { gridLayout } from "./layout.js";
import { parseObmesh } from "./mesh.js";
import { DEFAULTS, parseParams, suitePlan, suiteRunQuery } from "./params.js";
import { summarize } from "./stats.js";

const BASE = import.meta.env.BASE_URL;
/** One camera revolution per this many seconds; slow enough to look like orbiting a model. */
const ORBIT_PERIOD_S = 40;
/**
 * Frames always rendered before measuring (unless warmup=0), even when they take
 * longer than the warm-up time: drivers often compile pipelines at the first draws,
 * and that stall shows up one or two frames later.
 */
const MIN_WARMUP_FRAMES = 3;
/** Display colours (sRGB), chosen for contrast; not taken from any colour catalogue. */
const PALETTE = [0xc8323c, 0x2a62b8, 0xf2c230, 0xf0f0ec, 0x2b2f36, 0x2f8a4c, 0x9aa1a8, 0xe9772e];
const SOFTWARE_RE = /swiftshader|llvmpipe|lavapipe|softpipe|software|basic render/i;
const SUITE_KEY = "openbrick-harness-suite";

const $ = (id) => document.getElementById(id);

// ---------------------------------------------------------------- page state

const warnings = new Set();
let lastStatus = 0;

function setStatus(text, force = false) {
  const now = performance.now();
  if (!force && now - lastStatus < 500) return; // keep DOM work out of the measured frames
  lastStatus = now;
  $("status").textContent = text;
}

function fail(message) {
  $("status").className = "bad";
  $("status").textContent = message;
}

// ------------------------------------------------------------------- loading

async function fetchOk(url) {
  const res = await fetch(url);
  if (!res.ok) throw new Error(`could not load ${url}: HTTP ${res.status}`);
  return res;
}

/** Load the manifest and the kernel mesh of one LOD, cross-checking both. */
async function loadBrick(lod) {
  const manifest = await (await fetchOk(`${BASE}meshes/manifest.json`)).json();
  const entry = manifest.lods.find((l) => l.lod === lod);
  if (!entry) throw new Error(`mesh manifest has no LOD ${lod}`);
  const mesh = parseObmesh(await (await fetchOk(`${BASE}meshes/${entry.file}`)).arrayBuffer());
  if (mesh.triangles !== entry.triangles) {
    throw new Error(`${entry.file} has ${mesh.triangles} triangles, manifest says ${entry.triangles}; regenerate the meshes`);
  }
  if (mesh.units !== "mm" || mesh.upAxis !== "+Z") {
    throw new Error(`${entry.file} is ${mesh.units} with ${mesh.upAxis} up; the harness expects the kernel frame (mm, +Z up)`);
  }
  return { manifest, entry, mesh };
}

// --------------------------------------------------------------------- scene

/**
 * Build the scene. Everything below `world` stays in the kernel frame (millimetres,
 * right-handed, +Z up); `world` maps it to three.js (+Y up) with the rotation in
 * frame.js: (x, y, z) -> (x, z, -y), which keeps triangle winding.
 */
function buildScene({ manifest, mesh }, p) {
  const pitch = manifest.pitch_mm;
  const undersize = manifest.undersize_per_side_mm;
  const layout = gridLayout({ count: p.count, studs: manifest.studs, chunkSize: p.chunk });

  const geometry = new THREE.BufferGeometry();
  geometry.setAttribute("position", new THREE.BufferAttribute(mesh.positions, 3));
  geometry.setIndex(new THREE.BufferAttribute(mesh.indices, 1));
  // Kernel meshes share vertices across sharp edges and carry no normals, so the
  // material shades flat from screen-space derivatives instead of computing normals.
  const material = new THREE.MeshStandardNodeMaterial({ color: 0xffffff, roughness: 0.5, metalness: 0, flatShading: true });

  const scene = new THREE.Scene();
  scene.background = new THREE.Color(0xd9e1ea);
  scene.add(new THREE.HemisphereLight(0xffffff, 0x4a5563, 1.4));
  const sun = new THREE.DirectionalLight(0xffffff, 2.2);
  sun.position.set(0.45, 1, 0.3);
  scene.add(sun);

  const world = new THREE.Group();
  world.name = "kernel frame (mm, +Z up)";
  world.rotation.x = KERNEL_TO_THREE_EULER_X;
  scene.add(world);

  const m = new THREE.Matrix4();
  const colour = new THREE.Color();
  let index = 0;
  for (const xy of layout.chunks) {
    const n = xy.length / 2;
    const chunk = new THREE.InstancedMesh(geometry, material, n);
    for (let i = 0; i < n; i++) {
      // Lattice cell corner times the pitch, plus the undersize: the kernel puts the
      // body's minimum corner at the origin, one undersize inside the cell corner.
      m.makeTranslation(xy[2 * i] * pitch + undersize, xy[2 * i + 1] * pitch + undersize, 0);
      chunk.setMatrixAt(i, m);
      // Fibonacci hashing: the top bits of index x 2^32/phi spread colours evenly.
      const pick = Math.floor(((Math.imul(index++, 0x9e3779b1) >>> 0) / 2 ** 32) * PALETTE.length);
      chunk.setColorAt(i, colour.setHex(PALETTE[pick]));
    }
    chunk.instanceMatrix.needsUpdate = true;
    chunk.instanceColor.needsUpdate = true;
    chunk.computeBoundingSphere(); // per-chunk sphere, so frustum culling drops whole chunks
    world.add(chunk);
  }

  const extent = Math.max(layout.extent[0], layout.extent[1]) * pitch;
  const camera = new THREE.PerspectiveCamera(50, 1, extent / 200, extent * 4);
  const target = new THREE.Vector3(0, manifest.brick_height_mm / 2, 0);
  const orbit = (t) => {
    const a = Math.PI / 4 + (2 * Math.PI * t) / ORBIT_PERIOD_S;
    camera.position.set(Math.cos(a) * 0.6 * extent, 0.3 * extent, Math.sin(a) * 0.6 * extent);
    camera.lookAt(target);
  };
  orbit(0);
  return { scene, camera, orbit, layout };
}

// ------------------------------------------------------------------ renderer

async function adapterString(renderer) {
  const backend = renderer.backend;
  if (backend.isWebGPUBackend) {
    // GPUDevice.adapterInfo is newer than GPUAdapter.info; fall back to asking for an
    // adapter with the options three.js used (same browser, so the same GPU).
    let info = backend.device?.adapterInfo;
    if (!info) {
      const adapter = await navigator.gpu
        .requestAdapter({ powerPreference: "high-performance", featureLevel: "compatibility" })
        .catch(() => null);
      info = adapter?.info;
    }
    const parts = info ? [info.vendor, info.architecture, info.device, info.description] : [];
    const s = parts.filter(Boolean).join(" ");
    return s || "WebGPU adapter (details not exposed by the browser)";
  }
  const gl = backend.gl;
  const ext = gl.getExtension("WEBGL_debug_renderer_info");
  const vendor = gl.getParameter(ext ? ext.UNMASKED_VENDOR_WEBGL : gl.VENDOR);
  const name = gl.getParameter(ext ? ext.UNMASKED_RENDERER_WEBGL : gl.RENDERER);
  return [vendor, name].filter(Boolean).join(" / ");
}

async function createRenderer(p) {
  const renderer = new THREE.WebGPURenderer({
    antialias: p.aa === 1,
    forceWebGL: p.backend === "webgl2",
    powerPreference: "high-performance",
    // GPU timestamp queries (WebGPU 'timestamp-query', WebGL2 EXT_disjoint_timer_query)
    // only when asked for: the WebGL2 path polls for results on the main thread.
    trackTimestamp: p.gpu === 1,
  });
  renderer.setPixelRatio(Math.min(window.devicePixelRatio, p.dpr));
  renderer.setSize(window.innerWidth, window.innerHeight);
  $("stage").appendChild(renderer.domElement);
  await renderer.init();
  return renderer;
}

// --------------------------------------------------------------- measurement

/** True when GPU timing was requested and the backend has a GPU timer. */
function gpuTimer(renderer, p) {
  const backend = renderer.backend;
  return p.gpu === 1 && backend.trackTimestamp === true && (backend.isWebGPUBackend === true || Boolean(backend.disjoint));
}

/**
 * Render frames from requestAnimationFrame (renderer.setAnimationLoop drives rAF and
 * passes its timestamp). The first `warmup` seconds, and at least MIN_WARMUP_FRAMES
 * frames, are not recorded; after that every frame interval, JavaScript render time
 * and renderer.info count is recorded until `seconds` have passed.
 */
function measure(renderer, { scene, camera, orbit }, p, deviceLost) {
  return new Promise((resolve, reject) => {
    const rec = { frameMs: [], cpuMs: [], triangles: [], drawCalls: [], gpuMs: gpuTimer(renderer, p) ? [] : null };
    let gpuPending = false;
    let done = false;
    let t0 = null;
    let measureStart = null;
    let prev = null;
    let warmFrames = 0;
    deviceLost.catch((err) => {
      renderer.setAnimationLoop(null);
      reject(err);
    });
    renderer.setAnimationLoop((time) => {
      const now = time ?? performance.now();
      t0 ??= now;
      const t = (now - t0) / 1000;
      orbit(t);
      const c0 = performance.now();
      renderer.render(scene, camera);
      const cpu = performance.now() - c0;
      if (measureStart === null) {
        warmFrames++;
        setStatus(`Warming up ${t.toFixed(1)} / ${p.warmup} s`);
        if (t >= p.warmup && (p.warmup === 0 || warmFrames >= MIN_WARMUP_FRAMES)) {
          measureStart = now;
          prev = now;
        }
        return;
      }
      rec.frameMs.push(now - prev);
      rec.cpuMs.push(cpu);
      rec.triangles.push(renderer.info.render.triangles);
      rec.drawCalls.push(renderer.info.render.drawCalls);
      if (rec.gpuMs && !gpuPending) {
        // Resolves the GPU time of the most recent frame whose queries are ready.
        gpuPending = true;
        renderer
          .resolveTimestampsAsync(THREE.TimestampQuery.RENDER)
          .then((ms) => {
            if (!done && Number.isFinite(ms) && ms > 0) rec.gpuMs.push(ms);
          })
          .catch(() => {})
          .finally(() => {
            gpuPending = false;
          });
      }
      prev = now;
      const elapsed = (now - measureStart) / 1000;
      setStatus(`Measuring ${elapsed.toFixed(1)} / ${p.seconds} s, ${rec.frameMs.length} frames`);
      if (elapsed >= p.seconds) {
        done = true;
        renderer.setAnimationLoop(null);
        resolve({ ...rec, measuredMs: now - measureStart });
      }
    });
  });
}

// -------------------------------------------------------------------- result

/** A result object with every CSV column, filled from what is known so far. */
function baseResult(p) {
  const r = Object.fromEntries(COLUMN_NAMES.map((name) => [name, ""]));
  return Object.assign(r, {
    schema: SCHEMA_VERSION,
    device: p.device,
    backend_requested: p.backend,
    lod: p.lod,
    count: p.count,
    warmup: p.warmup,
    dpr_cap: p.dpr,
    device_pixel_ratio: window.devicePixelRatio,
    antialias: p.aa,
    three_revision: THREE.REVISION,
    user_agent: navigator.userAgent,
  });
}

function showResult(rows, p) {
  const r = rows[rows.length - 1];
  const csv = [csvHeader(), ...rows.map(csvRow)].join("\n") + "\n";
  window.__harnessCsv = csv;
  $("csv").value = csv;
  const facts = [
    ["fps median", r.fps_median],
    ["fps p5", r.fps_p5],
    ["frame ms p95", r.frame_ms_p95],
    ["triangles / frame", r.triangles_per_frame === "" ? "" : Number(r.triangles_per_frame).toLocaleString()],
    ["draw calls", r.draw_calls],
    ["backend", r.backend],
    ["canvas", r.canvas_width && `${r.canvas_width} x ${r.canvas_height}`],
    ["adapter", r.adapter],
  ];
  $("summary").replaceChildren(
    ...facts.map(([k, v]) => {
      const d = document.createElement("div");
      const s = document.createElement("span");
      s.textContent = k;
      d.append(s, String(v ?? ""));
      return d;
    }),
  );
  $("warnings").textContent = r.warnings ? `Warnings: ${r.warnings}` : "";
  $("result-config").textContent = rows.length > 1 ? `Suite of ${rows.length} runs; the summary shows the last one.` : $("config").textContent;
  $("hud").hidden = true;
  $("result").hidden = false;
  const name = rows.length > 1 ? "suite" : `${r.backend_requested}-lod${r.lod}-${r.count}`;
  $("download").onclick = () => {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(new Blob([csv], { type: "text/csv" }));
    a.download = `openbrick-harness-${name}.csv`;
    document.body.append(a);
    a.click();
    a.remove();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  };
  $("copy").onclick = async () => {
    try {
      await navigator.clipboard.writeText(csv);
    } catch {
      // The Clipboard API needs a secure context; a phone on http://<lan-ip> is not one.
      $("csv").select();
      document.execCommand("copy");
    }
    $("copied").textContent = "Copied.";
  };
  $("again").onclick = () => location.reload();
  fillSetup(p);
}

// -------------------------------------------------------------------- set-up

function fillSetup(p) {
  const form = $("setup");
  for (const key of ["count", "lod", "backend", "seconds", "warmup", "dpr", "aa", "gpu", "device"]) {
    form.elements[key].value = String(p[key]);
  }
  $("open-setup").onclick = () => {
    $("result").hidden = true;
    form.hidden = false;
    form.elements.count.focus();
  };
  $("close-setup").hidden = !window.__harnessCsv;
  $("close-setup").onclick = () => {
    form.hidden = true;
    $("result").hidden = false;
  };
  const read = () => Object.fromEntries(new FormData(form));
  form.onsubmit = (e) => {
    e.preventDefault();
    const q = new URLSearchParams(read());
    if (!q.get("device")) q.delete("device");
    location.search = q.toString();
  };
  $("suite").onclick = () => {
    const q = new URLSearchParams(read());
    for (const key of ["count", "lod", "backend"]) q.delete(key);
    if (!q.get("device")) q.delete("device");
    q.set("suite", "1");
    location.search = q.toString();
  };
}

// --------------------------------------------------------------------- suite

function suiteStore() {
  try {
    const s = window.sessionStorage;
    s.setItem(`${SUITE_KEY}-probe`, "1");
    s.removeItem(`${SUITE_KEY}-probe`);
    return {
      get: (sid) => JSON.parse(s.getItem(`${SUITE_KEY}-${sid}`) ?? "[]"),
      set: (sid, rows) => s.setItem(`${SUITE_KEY}-${sid}`, JSON.stringify(rows)),
    };
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------- main

async function main() {
  const { params: p, errors } = parseParams(location.search);
  const inSuite = p.suite === 1;
  const plan = inSuite ? suitePlan(p) : [];
  $("config").textContent =
    `${p.count.toLocaleString()} bricks, LOD${p.lod}, ${p.backend}, ${p.seconds} s after ${p.warmup} s warm-up, DPR cap ${p.dpr}` +
    (inSuite && p.run >= 0 ? ` (suite run ${p.run + 1} of ${plan.length})` : "");
  if (errors.length > 0) throw new Error(`Bad URL parameters:\n${errors.join("\n")}`);

  const store = inSuite ? suiteStore() : null;
  if (inSuite && !store) throw new Error("Suite mode needs sessionStorage, which this browser blocks; run the URLs one at a time instead.");
  if (inSuite && p.run < 0) {
    const sid = Date.now().toString(36);
    store.set(sid, []);
    location.replace(`?${suiteRunQuery(p, 0, sid)}`);
    return;
  }

  const result = baseResult(p);
  let rows = [result];
  try {
    if (!window.isSecureContext && p.backend === "webgpu") {
      warnings.add("page is not a secure context, so WebGPU is unavailable (see bench/devices/README.md)");
    }
    const brick = await loadBrick(p.lod);
    Object.assign(result, {
      triangles_per_brick: brick.entry.triangles,
      kernel: brick.manifest.kernel,
      mesh_fingerprint: brick.entry.fingerprint,
    });
    setStatus("Starting renderer…", true);
    const renderer = await createRenderer(p);
    // A lost GPU device (driver reset, watchdog on a slow phone frame, WebGL context
    // loss) ends the run with an error row instead of a hung page.
    const deviceLost = new Promise((_, reject) => {
      renderer.onDeviceLost = (info) => reject(new Error(`${info.api} device lost: ${info.message}`));
    });
    deviceLost.catch(() => {});
    const backend = renderer.backend.isWebGPUBackend ? "webgpu" : "webgl2";
    const adapter = await adapterString(renderer);
    Object.assign(result, { backend, adapter, software: SOFTWARE_RE.test(adapter) ? "yes" : "no" });
    if (backend !== p.backend) warnings.add(`asked for ${p.backend}, got ${backend}`);
    if (renderer.backend.compatibilityMode) warnings.add("WebGPU compatibility mode (no MSAA)");
    if (result.software === "yes") warnings.add("software rasteriser: not a device result");
    if (p.gpu === 1 && !gpuTimer(renderer, p)) warnings.add("no GPU timer in this browser");

    const scene = buildScene(brick, p);
    const resize = () => {
      renderer.setSize(window.innerWidth, window.innerHeight);
      scene.camera.aspect = window.innerWidth / window.innerHeight;
      scene.camera.updateProjectionMatrix();
    };
    resize();
    let measuring = false;
    window.addEventListener("resize", () => {
      resize();
      if (measuring) warnings.add("window resized during measurement");
    });
    document.addEventListener("visibilitychange", () => {
      if (measuring && document.hidden) warnings.add("tab hidden during measurement");
    });
    setStatus("Compiling shaders…", true);
    await Promise.race([renderer.compileAsync(scene.scene, scene.camera), deviceLost]);
    measuring = true;
    const rec = await measure(renderer, scene, p, deviceLost);
    measuring = false;
    const stats = summarize(rec);
    if (stats.frames < 30) warnings.add(`only ${stats.frames} frames measured`);
    if (stats.gpu_samples === 0) warnings.add("GPU timer gave no readings");
    Object.assign(result, stats, {
      chunks: scene.layout.chunks.length,
      chunk_size: scene.layout.chunkCapacity,
      pixel_ratio: renderer.getPixelRatio(),
      canvas_width: renderer.domElement.width,
      canvas_height: renderer.domElement.height,
    });
    setStatus(`Done: ${stats.fps_median} fps median over ${stats.frames} frames.`, true);
  } catch (err) {
    warnings.add(`error: ${err.message}`);
    result.error = err.message;
    fail(err.message);
  } finally {
    result.timestamp = new Date().toISOString();
    result.warnings = [...warnings].join("; ");
  }

  if (inSuite) {
    rows = store.get(p.sid);
    rows[p.run] = result;
    store.set(p.sid, rows);
    if (p.run + 1 < plan.length) {
      setStatus(`Suite run ${p.run + 1} of ${plan.length} done; next run in 1 s…`, true);
      window.__harnessResult = result;
      setTimeout(() => location.replace(`?${suiteRunQuery(p, p.run + 1, p.sid)}`), 1000);
      return;
    }
    rows = rows.filter(Boolean);
    window.__harnessSuite = rows;
  }
  window.__harnessResult = result;
  showResult(rows, p);
  window.__harnessDone = true;
}

main().catch((err) => {
  fail(err.message);
  fillSetup({ ...DEFAULTS, ...parseParams(location.search).params });
  $("setup").hidden = false;
  window.__harnessResult = { error: err.message };
  window.__harnessDone = true;
});

