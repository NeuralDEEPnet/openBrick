// Frame-time statistics. Pure functions so they run under node --test.

/**
 * Percentile of an ascending array with linear interpolation between closest ranks
 * (the "linear" method of NumPy and R type 7). p is in [0, 100].
 */
export function percentile(sorted, p) {
  if (sorted.length === 0) return NaN;
  const rank = (p / 100) * (sorted.length - 1);
  const lo = Math.floor(rank);
  const hi = Math.ceil(rank);
  return sorted[lo] + (sorted[hi] - sorted[lo]) * (rank - lo);
}

const ascending = (a, b) => a - b;

/** Round to a fixed number of decimals for the CSV (keeps rows short and diffable). */
export function round(x, decimals = 2) {
  if (!Number.isFinite(x)) return x;
  const f = 10 ** decimals;
  return Math.round(x * f) / f;
}

/**
 * Summarise a measurement.
 * frameMs: intervals between successive requestAnimationFrame timestamps.
 * cpuMs: JavaScript time spent inside renderer.render() per frame.
 * triangles, drawCalls: renderer.info values per frame.
 * measuredMs: wall time from the start of measurement to the last frame.
 * gpuMs: optional GPU timer readings in milliseconds; null when GPU timing is off.
 */
export function summarize({ frameMs, cpuMs, triangles, drawCalls, measuredMs, gpuMs = null }) {
  const ms = [...frameMs].sort(ascending);
  const fps = frameMs.map((t) => 1000 / t).sort(ascending);
  const cpu = [...cpuMs].sort(ascending);
  const gpu = gpuMs ? [...gpuMs].sort(ascending) : null;
  const gpuStat = (q) => (gpu && gpu.length > 0 ? round(percentile(gpu, q), 3) : "");
  return {
    frames: frameMs.length,
    seconds: round(measuredMs / 1000, 3),
    fps_median: round(percentile(fps, 50)),
    fps_mean: round((frameMs.length * 1000) / measuredMs),
    fps_p5: round(percentile(fps, 5)),
    frame_ms_p50: round(percentile(ms, 50)),
    frame_ms_p95: round(percentile(ms, 95)),
    frame_ms_max: round(ms[ms.length - 1]),
    cpu_ms_p50: round(percentile(cpu, 50), 3),
    cpu_ms_p95: round(percentile(cpu, 95), 3),
    gpu_samples: gpu ? gpu.length : "",
    gpu_ms_p50: gpuStat(50),
    gpu_ms_p95: gpuStat(95),
    triangles_per_frame: Math.round(percentile([...triangles].sort(ascending), 50)),
    draw_calls: Math.round(percentile([...drawCalls].sort(ascending), 50)),
  };
}
