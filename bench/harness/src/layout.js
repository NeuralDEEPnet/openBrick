// Lattice layout of N bricks in a square grid, grouped into spatially compact chunks.
// Positions are integer stud counts (the lattice); the caller multiplies by the
// pitch from the mesh manifest, which the kernel took from standards/dimensions-v1.json.

/** Empty studs left between neighbouring bricks so their side walls are visible. */
export const GAP_STUDS = 1;

/**
 * Lay out `count` bricks with a footprint of `studs` = [x, y] studs.
 * The grid has about the same extent in x and y. Bricks are grouped by square tiles
 * of side floor(sqrt(chunkSize)) so that each chunk is compact and its bounding
 * sphere is tight, which is what makes per-chunk frustum culling effective.
 *
 * Returns { cols, rows, spacing, extent, tileSide, chunks } where spacing and
 * extent are in studs and each chunk is an Int32Array of (x, y) stud offsets of
 * brick minimum corners, centred on the origin.
 */
export function gridLayout({ count, studs, chunkSize, gapStuds = GAP_STUDS }) {
  if (!(Number.isInteger(count) && count > 0)) throw new RangeError(`count must be a positive integer, got ${count}`);
  if (!(Number.isInteger(chunkSize) && chunkSize > 0)) throw new RangeError(`chunkSize must be a positive integer, got ${chunkSize}`);
  const spacing = [studs[0] + gapStuds, studs[1] + gapStuds];
  // cols * spacing[0] ~ rows * spacing[1] and cols * rows >= count.
  const cols = Math.max(1, Math.ceil(Math.sqrt((count * spacing[1]) / spacing[0])));
  const rows = Math.ceil(count / cols);
  const extent = [cols * spacing[0], rows * spacing[1]];
  // Integer centring keeps every brick on the stud lattice.
  const origin = [Math.floor(extent[0] / 2), Math.floor(extent[1] / 2)];
  const tileSide = Math.max(1, Math.floor(Math.sqrt(chunkSize)));
  const chunks = [];
  for (let tr = 0; tr < rows; tr += tileSide) {
    for (let tc = 0; tc < cols; tc += tileSide) {
      const xy = [];
      for (let r = tr; r < Math.min(tr + tileSide, rows); r++) {
        for (let c = tc; c < Math.min(tc + tileSide, cols); c++) {
          if (r * cols + c >= count) continue;
          xy.push(c * spacing[0] - origin[0], r * spacing[1] - origin[1]);
        }
      }
      if (xy.length > 0) chunks.push(Int32Array.from(xy));
    }
  }
  return { cols, rows, spacing, extent, tileSide, chunkCapacity: tileSide * tileSide, chunks };
}
