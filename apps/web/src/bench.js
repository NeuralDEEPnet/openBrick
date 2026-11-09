// `?bench=1`: the scale benchmark on the device in front of you (bench/scale/README.md, docs/plan/07
// section 9). It opens generated models of increasing size in Build mode, orbits the camera for a few
// seconds at each size and prints one CSV row per size, to copy or download and send back, so the
// performance budgets can be checked on real laptops and phones, not in software rendering.
//
//   ?bench=1&sizes=1000,10000,100000&layout=city&seconds=8&device=Pixel%208a%20Chrome
//
// The pure parts (the columns, the CSV text, the percentiles, the URL's options) are here and are tested
// with node --test; the page part (mountBench) builds a small panel.

/** One CSV column each: name, and where the value is read from in a result row. */
export const CSV_COLUMNS = [
  "date",
  "device",
  "user_agent",
  "gpu",
  "backend",
  "device_pixel_ratio",
  "pixel_ratio_end",
  "viewport",
  "layout",
  "bricks",
  "parts",
  "open_core_ms",
  "open_page_ms",
  "chunks",
  "draw_calls",
  "triangles",
  "chunks_full",
  "chunks_cheap",
  "chunks_solid",
  "chunks_hull",
  "frames",
  "frame_ms_p50",
  "frame_ms_p95",
  "fps_mean",
  "snap_ms",
  "pick_ms",
  "place_undo_ms",
  "heap_mib",
  "warnings",
];

/** The defaults and the limits of the URL's options. */
export const BENCH_DEFAULTS = { sizes: [1000, 10000, 100000], layout: "city", seconds: 8, device: "" };
export const MAX_BENCH_BRICKS = 140_000;

/** The options of `?bench=1&sizes=..&layout=..&seconds=..&device=..`; `enabled` is false without bench=1. */
export function benchOptions(search) {
  const p = new URLSearchParams(search);
  const sizes = (p.get("sizes") ?? "")
    .split(",")
    .map((s) => {
      const m = /^(\d+)(k?)$/i.exec(s.trim());
      return m ? Number(m[1]) * (m[2] ? 1000 : 1) : NaN;
    })
    .filter((n) => Number.isInteger(n) && n >= 1 && n <= MAX_BENCH_BRICKS);
  const seconds = Number(p.get("seconds"));
  const layout = ["towers", "plates", "city"].includes(p.get("layout")) ? p.get("layout") : BENCH_DEFAULTS.layout;
  return {
    enabled: p.get("bench") === "1",
    sizes: sizes.length ? sizes : BENCH_DEFAULTS.sizes,
    layout,
    seconds: seconds >= 1 && seconds <= 60 ? seconds : BENCH_DEFAULTS.seconds,
    device: (p.get("device") ?? "").slice(0, 80),
  };
}

/** The value at fraction `p` (0 to 1) of the numbers, 0 for none. */
export function percentile(values, p) {
  if (!values.length) return 0;
  const s = [...values].sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(s.length * p))];
}

/** Frame statistics of the gaps between frames, in ms: { frames, p50, p95, fps }. */
export function frameStats(times) {
  if (!times.length) return { frames: 0, p50: 0, p95: 0, fps: 0 };
  const mean = times.reduce((a, b) => a + b, 0) / times.length;
  return { frames: times.length, p50: percentile(times, 0.5), p95: percentile(times, 0.95), fps: 1000 / mean };
}

/** One CSV field: quoted when it holds a comma, a quote or a line break. */
export function csvField(value) {
  const text = value === null || value === undefined ? "" : String(value);
  return /[",\n\r]/.test(text) ? `"${text.replaceAll('"', '""')}"` : text;
}

/** The CSV header line. */
export const csvHeader = () => CSV_COLUMNS.join(",");

/** One result row (an object keyed by [`CSV_COLUMNS`]) as a CSV line. */
export function csvLine(row) {
  return CSV_COLUMNS.map((c) => csvField(row[c])).join(",");
}

/** Warnings for a run that may not mean what it seems: a hidden tab, a software renderer, a run cut short. */
export function benchWarnings({ hidden, gpu, backend, fpsMean, frames, error }) {
  const w = [];
  if (error) w.push(`error: ${error}`);
  if (hidden) w.push("the page was hidden during the run, so frames were paused");
  if (/swiftshader|llvmpipe|software|basic render/i.test(gpu ?? "")) w.push("software renderer: not a device number");
  if (backend === "webgl2" && !gpu) w.push("the GPU name is hidden by the browser");
  if (frames < 30) w.push("fewer than 30 frames measured");
  if (fpsMean > 0 && fpsMean < 5) w.push("under 5 fps: the model is too big for this device at this size");
  return w;
}

/** The text a person reads under the table: how to send the result back. */
export const BENCH_HELP =
  "When the table is complete, press Copy CSV (or Download CSV) and send the rows to the project (bench/devices/README.md says where). " +
  "Keep this tab in front and the screen on while it runs; a hidden tab pauses frames and the row says so.";

/**
 * Show the benchmark panel over the page and run `run(options, onRow)`; `run` resolves with all rows. The panel
 * has the rows as a table and the CSV as text, with Copy and Download buttons.
 */
export function mountBench({ options, run, document: doc = document }) {
  const panel = doc.createElement("section");
  panel.id = "bench";
  panel.setAttribute("aria-label", "Scale benchmark");
  panel.style.cssText =
    "position:fixed;inset:auto 8px 8px 8px;z-index:50;max-height:55vh;overflow:auto;padding:10px 12px;border-radius:10px;font:12px/1.4 ui-monospace,Menlo,Consolas,monospace;background:rgba(16,22,29,.94);color:#e6ecf2";
  const title = doc.createElement("strong");
  title.textContent = `Scale benchmark: ${options.layout}, ${options.sizes.map((n) => n.toLocaleString("en-US")).join(", ")} bricks, ${options.seconds} s each`;
  const status = doc.createElement("div");
  status.id = "bench-status";
  status.setAttribute("role", "status");
  const help = doc.createElement("div");
  help.textContent = BENCH_HELP;
  const csv = doc.createElement("pre");
  csv.id = "bench-csv";
  csv.style.cssText = "margin:8px 0;white-space:pre-wrap;word-break:break-all;user-select:all";
  const bar = doc.createElement("div");
  const button = (label, id, on) => {
    const b = doc.createElement("button");
    b.type = "button";
    b.id = id;
    b.textContent = label;
    b.style.cssText = "margin-right:8px;padding:6px 10px;min-height:36px";
    b.addEventListener("click", on);
    bar.append(b);
    return b;
  };
  const lines = [csvHeader()];
  const show = () => {
    csv.textContent = lines.join("\n");
  };
  button("Copy CSV", "bench-copy", async () => {
    try {
      await navigator.clipboard.writeText(`${lines.join("\n")}\n`);
      status.textContent = "Copied.";
    } catch {
      status.textContent = "Copy was refused: select the text below and copy it.";
    }
  });
  button("Download CSV", "bench-download", () => {
    const url = URL.createObjectURL(new Blob([`${lines.join("\n")}\n`], { type: "text/csv" }));
    const a = doc.createElement("a");
    a.href = url;
    a.download = `openbrick-scale-${options.layout}-${new Date().toISOString().slice(0, 10)}.csv`;
    a.click();
    setTimeout(() => URL.revokeObjectURL(url), 10_000);
  });
  panel.append(title, status, help, bar, csv);
  doc.body.append(panel);
  show();
  const onRow = (row) => {
    lines.push(csvLine(row));
    show();
  };
  const onStatus = (text) => {
    status.textContent = text;
  };
  return run(options, onRow, onStatus).then(
    (rows) => {
      onStatus(`Done: ${rows.length} row${rows.length === 1 ? "" : "s"}.`);
      doc.documentElement.dataset.benchDone = "1";
      return rows;
    },
    (err) => {
      onStatus(`Stopped: ${err.message}`);
      doc.documentElement.dataset.benchDone = "error";
      throw err;
    },
  );
}
