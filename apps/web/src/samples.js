// The artwork samples the core bundles (`artwork_samples_json()` in
// crates/openbrick-wasm/src/data.rs): the NEURALDEEP.NET wordmark (the
// playground's default), the square NEURALDEEP.NET wordmark and the CC0 sample
// mark. These pure functions say which of a sample's drawings the page sends
// for a tile, and which tile a sample asks for when it is chosen. They pick data; they compute
// no geometry (CLAUDE.md), so node --test checks them.

/**
 * Whether a [width, length] tile has the shape a sample is drawn for:
 * `portrait` is longer than wide, `square` is square, anything else (null) is
 * any tile.
 */
export function fitsShape(shape, [w, l]) {
  if (shape === "square") return w === l;
  if (shape === "portrait") return w < l;
  return true;
}

const area = (f) => (Array.isArray(f.tile) ? f.tile[0] * f.tile[1] : 0);

/** Whether a drawing fits a tile: drawn for any tile, or for one no wider and no longer. */
const fitsTile = (f, size) => !Array.isArray(f.tile) || (Array.isArray(size) && f.tile[0] <= size[0] && f.tile[1] <= size[1]);

/** Whether a [width, length] tile is smaller than every drawing of `sample`. */
export function tooSmall(sample, size) {
  const files = sample?.files ?? [];
  return files.length > 0 && Array.isArray(size) && !files.some((f) => fitsTile(f, size));
}

/**
 * The tile to switch to when `sample` is chosen on a tile of `size`: null when
 * the tile has the sample's shape and at least one of its drawings fits it,
 * else the sample's own size (its smallest drawing's tile). So the wordmark on
 * a square, wide or 2x4 tile asks for 2x6 (the whole text does not print
 * smaller) and the square wordmark on a tile that is not square, or on 2x2,
 * asks for 4x4; later size changes are the user's.
 */
export function sizeForSample(sample, size) {
  if (!Array.isArray(sample?.size) || !Array.isArray(size)) return null;
  if (fitsShape(sample.shape, size) && !tooSmall(sample, size)) return null;
  return [...sample.size];
}

/**
 * The drawing of `sample` the page sends for a tile of `size`: the one drawn
 * for the largest tile that fits inside it (its width and its length each at
 * least the drawing's), else the one drawn for the smallest tile. A drawing
 * with no tile is for any tile. On a 2x8 or 4x8 tile the NEURALDEEP.NET
 * wordmark is the drawing made for 2x8; on 2x6, 3x7 or 4x6 the one made for
 * 2x6, which is also sent, scaled down, on a tile smaller than that.
 */
export function sampleFile(sample, size) {
  const files = sample?.files ?? [];
  if (files.length === 0) throw new Error(`the sample ${sample?.id} has no drawing`);
  const fits = files.filter((f) => fitsTile(f, size));
  if (fits.length > 0) return fits.reduce((best, f) => (area(f) > area(best) ? f : best));
  return files.reduce((best, f) => (area(f) < area(best) ? f : best));
}

/** The drawing in a sentence: "the NEURALDEEP.NET wordmark (drawn for 2x6)". */
export function sampleLabel(sample, file) {
  return Array.isArray(file?.tile) ? `${sample.title} (drawn for ${file.tile.join("x")})` : sample.title;
}

/**
 * The note shown when choosing `sample` on a tile of `before` set the tile to
 * `size`: the tile had another shape, or it was too small for the drawing.
 */
export function sizeNote(sample, size, before) {
  const title = sample.title.charAt(0).toUpperCase() + sample.title.slice(1);
  if (Array.isArray(before) && fitsShape(sample.shape, before)) {
    return `${title} needs a tile of at least ${size.join(" × ")} to print, so the size is now ${size.join(" × ")}.`;
  }
  const tiles = sample.shape === "square" ? "square tiles" : "tiles longer than they are wide";
  return `${title} is drawn for ${tiles}, so the size is now ${size.join(" × ")}.`;
}
