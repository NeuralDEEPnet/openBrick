// Deterministic large models for the scale benchmark (docs/plan/07 section 14, bench/scale/README.md):
// the same few lines everywhere (node script, browser run, tests), no randomness beyond a seeded LCG.
//
// Every model is made of built-in parts on 16 x 16 baseplates laid out in a square grid, 320 LDU
// apart (a baseplate is 320 LDU wide, a 2x4 brick 80 by 40, so 4 by 8 of them tile one baseplate).
// Positions are integer LDU and the rotation is 0; the core still checks every part when it opens
// the text (known part, stud grid, above the ground, no overlap).

export const LAYOUTS = ["towers", "plates", "city"];

const PLATE_PITCH = 320;
const COLUMNS = [-120, -40, 40, 120];
const ROWS = [...Array(8).keys()].map((r) => -140 + 40 * r);
const COLOURS = [10000, 15, 4, 14, 2, 25, 71, 72, 0, 1, 19, 70];

/** A small seeded generator: deterministic across runs and machines. */
export function lcg(seed) {
  let s = seed >>> 0 || 1;
  return () => {
    s = (Math.imul(s, 1664525) + 1013904223) >>> 0;
    return s / 4294967296;
  };
}

/**
 * The instances of a model of about `bricks` bricks (plus its baseplates): `layout` is "towers"
 * (16 layers of 2x4 bricks over each baseplate), "plates" (16 layers of 2x4 plates) or "city" (a
 * seeded random skyline: each of the 32 footprints of a baseplate gets a column of 1 to 24 bricks
 * and plates).
 * @returns {{ instances: object[], bricks: number, baseplates: number }}
 */
export function layoutInstances(layout, bricks, seed = 1) {
  if (!LAYOUTS.includes(layout)) throw new RangeError(`unknown layout ${layout}`);
  const rand = lcg(seed);
  const instances = [];
  let id = 1;
  const add = (part, x, y, z, colour) => instances.push({ id: id++, part, pose: { pos: [x, y, z], rot: 0 }, colour });
  const perPlateLayers = layout === "city" ? 12 : 16;
  const baseplates = Math.max(1, Math.ceil(bricks / (32 * perPlateLayers)));
  const side = Math.ceil(Math.sqrt(baseplates));
  let placed = 0;
  for (let b = 0; b < baseplates && placed < bricks; b++) {
    const ox = (b % side) * PLATE_PITCH;
    const oz = Math.floor(b / side) * PLATE_PITCH;
    add("ob.baseplate-16x16", ox, -8, oz, 2);
    let spot = 0;
    for (const x of COLUMNS) {
      for (const z of ROWS) {
        spot += 1;
        if (layout === "city") {
          const height = 1 + Math.floor(rand() * 2 * perPlateLayers);
          let y = -8;
          for (let k = 0; k < height && placed < bricks; k++) {
            const plate = rand() < 0.25;
            y -= plate ? 8 : 24;
            add(plate ? "ob.plate-2x4" : "ob.brick-2x4", ox + x, y, oz + z, COLOURS[Math.floor(rand() * COLOURS.length)]);
            placed += 1;
          }
        } else {
          const brick = layout === "towers";
          for (let layer = 0; layer < perPlateLayers && placed < bricks; layer++) {
            const y = -8 - (brick ? 24 : 8) * (layer + 1);
            add(brick ? "ob.brick-2x4" : "ob.plate-2x4", ox + x, y, oz + z, COLOURS[(layer + spot) % COLOURS.length]);
            placed += 1;
          }
        }
      }
    }
  }
  return { instances, bricks: placed, baseplates };
}

/** The `.obrick` text of a layout, in the form the core writes (the core normalises it on save). */
export function layoutText(layout, bricks, seed = 1) {
  const { instances } = layoutInstances(layout, bricks, seed);
  return JSON.stringify({
    openbrick: "1.0",
    kind: "model",
    name: `${layout} ${bricks}`,
    credit: { author: "scale benchmark" },
    units: "ldu",
    frame: "ldraw-y-down",
    catalog: "openbrick-builtin@1",
    palette: [],
    submodels: [{ id: "main", instances }],
    bom: [],
  });
}

/** Rays from above over the model's footprint, for the snap and pick timings: [{ origin, dir }]. */
export function probeRays(instances, count, seed = 7) {
  const rand = lcg(seed);
  const xs = instances.map((i) => i.pose.pos[0]);
  const zs = instances.map((i) => i.pose.pos[2]);
  const [x0, x1] = [Math.min(...xs.slice(0, 1e5)), Math.max(...xs.slice(0, 1e5))];
  const [z0, z1] = [Math.min(...zs.slice(0, 1e5)), Math.max(...zs.slice(0, 1e5))];
  return Array.from({ length: count }, () => ({
    origin: [x0 + rand() * (x1 - x0), -3000, z0 + rand() * (z1 - z0)],
    dir: [0.15 * (rand() - 0.5), 1, 0.15 * (rand() - 0.5)],
  }));
}
