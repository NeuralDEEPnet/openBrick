// The artwork samples the core bundles (`artwork_samples_json()` in
// crates/openbrick-wasm/src/data.rs): the Neuraldeep.net mark (the playground's
// default), the Neuraldeep.net badge and the CC0 sample mark. These pure
// functions say which of a sample's drawings the page sends for a tile, and
// which tile a sample asks for when it is chosen. They pick data; they compute
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

/**
 * The tile to switch to when `sample` is chosen on a tile of `size`: null when
 * the tile already has the sample's shape, else the sample's own size. So the
 * portrait mark on a square (or wide) tile asks for 2x4 and the badge on a tile
 * that is not square asks for 4x4; later size changes are the user's.
 */
export function sizeForSample(sample, size) {
  if (!Array.isArray(sample?.size) || !Array.isArray(size) || fitsShape(sample.shape, size)) return null;
  return [...sample.size];
}

const area = (f) => (Array.isArray(f.tile) ? f.tile[0] * f.tile[1] : 0);

/**
 * The drawing of `sample` the page sends for a tile of `size`: the one drawn
 * for the largest tile that fits inside it (its width and its length each at
 * least the drawing's), else the one drawn for the smallest tile. A drawing
 * with no tile is for any tile. On a 4x6 tile the Neuraldeep.net mark is the
 * drawing made for 4x6; on 2x4, 3x5 or 6x4 it is the one made for 2x4.
 */
export function sampleFile(sample, size) {
  const files = sample?.files ?? [];
  if (files.length === 0) throw new Error(`the sample ${sample?.id} has no drawing`);
  const fits = files.filter((f) => !Array.isArray(f.tile) || (Array.isArray(size) && f.tile[0] <= size[0] && f.tile[1] <= size[1]));
  if (fits.length > 0) return fits.reduce((best, f) => (area(f) > area(best) ? f : best));
  return files.reduce((best, f) => (area(f) < area(best) ? f : best));
}

/** The drawing in a sentence: "the Neuraldeep.net mark (drawn for 2x4)". */
export function sampleLabel(sample, file) {
  return Array.isArray(file?.tile) ? `${sample.title} (drawn for ${file.tile.join("x")})` : sample.title;
}

/** The note shown when choosing `sample` set the tile to `size`. */
export function sizeNote(sample, size) {
  const tiles = sample.shape === "square" ? "square tiles" : "tiles longer than they are wide";
  const title = sample.title.charAt(0).toUpperCase() + sample.title.slice(1);
  return `${title} is drawn for ${tiles}, so the size is now ${size.join(" × ")}.`;
}
