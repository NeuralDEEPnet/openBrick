// The parts palette of Build mode for hundreds of parts: search, category filter,
// pages, badges, the attribution line and the text for a model that needs a part
// pack, plus a small least-recently-used cache for the meshes the page holds. Pure
// logic with no DOM and no three.js, so node --test checks it. Nothing here decides
// where a part goes or what it connects to: the part list, its categories and its
// connectivity flags all come from the core (docs/plan/07 section 3); this only
// chooses what to show of them.

/** Parts shown per page of the palette. */
export const PAGE_SIZE = 24;

/** The most meshes (CPU copies, for thumbnails and instancing) kept at once beyond those in use. */
export const MESH_CACHE_SIZE = 48;

/** The most part types with a GPU mesh at once beyond the placed ones and the ghost. */
export const GPU_RESIDENT_SPARE = 32;

const norm = (s) => String(s ?? "").toLowerCase().replace(/\s+/g, " ").trim();

/**
 * A search text as the pieces a part must contain: "brick 2x4" is the word "brick" and the size
 * "2 x 4" (digits with an x between them are one size, written the way part names write it).
 */
export function searchTokens(query) {
  const q = norm(query).replace(/(\d+)\s*[x×]\s*(\d+)/g, "\u0001$1 x $2\u0001");
  return q
    .split("\u0001")
    .flatMap((piece, i) => (i % 2 === 1 ? [piece] : piece.split(" ")))
    .filter(Boolean);
}

/** The words a part is searched by: its name (size spelled as "2 x 4"), id and category, with a space in front so a piece matches at the start of a word. */
function haystack(part) {
  return ` ${norm(`${part.name} ${part.id} ${part.category ?? ""}`).replace(/(\d+)\s*[x×]\s*(\d+)/g, "$1 x $2")}`;
}

/**
 * The parts of `parts` (the core's list) that match `query` and `category`, as indexes in
 * the order of the list. `category` is a name or "" for all; `connectivity` is "" for all or
 * "derived" or "none".
 */
export function filterParts(parts, { query = "", category = "", connectivity = "" } = {}) {
  const tokens = searchTokens(query);
  const out = [];
  parts.forEach((p, i) => {
    if (category && (p.category ?? "") !== category) return;
    if (connectivity && (p.connectivity ?? "derived") !== connectivity) return;
    if (tokens.length) {
      const h = haystack(p);
      if (!tokens.every((t) => h.includes(` ${t}`))) return;
    }
    out.push(i);
  });
  return out;
}

/** The categories of the parts, with how many parts each has: [{ name, count }] sorted by name. */
export function categoriesOf(parts) {
  const counts = new Map();
  for (const p of parts) {
    const c = p.category ?? "";
    if (c) counts.set(c, (counts.get(c) ?? 0) + 1);
  }
  return [...counts].map(([name, count]) => ({ name, count })).sort((a, b) => a.name.localeCompare(b.name));
}

/** The number of pages of `count` items. At least one, so an empty list still has a page 1. */
export function pageCount(count, size = PAGE_SIZE) {
  return Math.max(1, Math.ceil(count / size));
}

/** The items of page `page` (0-based, clamped) of `list`, and the page used. */
export function pageOf(list, page, size = PAGE_SIZE) {
  const last = pageCount(list.length, size) - 1;
  const p = Math.min(Math.max(0, Math.trunc(page) || 0), last);
  return { page: p, pages: last + 1, items: list.slice(p * size, p * size + size) };
}

/** The page that holds the item at position `position` of the filtered list. */
export function pageHolding(position, size = PAGE_SIZE) {
  return Math.max(0, Math.floor(position / size));
}

/** The status text of the list: "Showing 1 to 24 of 1,371 parts". */
export function rangeText(total, page, size = PAGE_SIZE) {
  if (total === 0) return "No part matches: clear the search or choose All categories";
  const from = page * size + 1;
  const to = Math.min(total, from + size - 1);
  return `Showing ${from} to ${to} of ${total.toLocaleString("en-US")} part${total === 1 ? "" : "s"}`;
}

/**
 * The note a part carries in the palette, or null: parts without a connector can only
 * stand on the ground (the engine will not attach them to anything), and clip, hinge, ball and
 * socket joints are not modelled yet.
 */
export function partNote(part) {
  if ((part.connectivity ?? "derived") === "none") return "Free placement only: no connector was found, so it can only stand on the ground";
  if (part.joints?.length) return `Studs snap; the ${part.joints.join(", ")} joint is not modelled yet`;
  return null;
}

/** A short badge for the list, or "". */
export function partBadge(part) {
  if ((part.connectivity ?? "derived") === "none") return "ground only";
  if (part.joints?.length) return "joint not modelled";
  return "";
}

/** The attribution line the page shows while an LDraw pack is loaded, or "". */
export function attributionLine(pack) {
  if (!pack) return "";
  return `${pack.attribution} Library ${pack.library}, ${pack.parts} parts. CC BY 2.0 / CC BY 4.0, credits per part below.`;
}

/** What a part detail says about its source: "Author: …; Licensed under …". */
export function creditLine(part) {
  if (!part.author && !part.license) return "";
  return [part.author && `Author: ${part.author}`, part.license].filter(Boolean).join(" · ");
}

/**
 * What to tell a person who opens a model that needs parts the core does not have, from
 * `assembler_model_needs`: { requires, missing, missingCount }. Null when nothing is missing.
 */
export function missingPackText(needs, packLoaded) {
  if (!needs || needs.missingCount === 0) return null;
  const n = needs.missingCount;
  const need = needs.requires?.[0];
  const which = need ? `the ${need.pack} pack (library ${need.library})` : "an LDraw parts pack";
  const parts = needs.missing.slice(0, 3).join(", ");
  if (packLoaded) {
    return `This model uses ${n} part${n === 1 ? "" : "s"} that the loaded pack does not have (${parts}…). It needs ${which}: add a pack made from that library.`;
  }
  return `This model uses ${n} part${n === 1 ? "" : "s"} from ${which} (${parts}…). Add parts from an LDraw library to open it.`;
}

/** Whether a part id belongs to an LDraw pack. */
export const isPackPart = (id) => String(id).startsWith("ld.");

/** A pack file's name, shortened for a status line. */
export function packFileLabel(name) {
  return String(name ?? "pack").replace(/\.obpack$/i, "");
}

/** Whether a file looks like a pack: .obpack. */
export function isPackFile(name) {
  return /\.obpack$/i.test(String(name ?? ""));
}

/**
 * A least-recently-used cache of part meshes. `keep(index)` says which entries must stay
 * (placed parts and the one being placed); only the others are dropped, oldest first, when
 * more than `size` of them are held.
 */
export class MeshCache {
  #map = new Map();
  #size;

  constructor(size = MESH_CACHE_SIZE) {
    this.#size = size;
  }

  get size() {
    return this.#map.size;
  }

  has(index) {
    return this.#map.has(index);
  }

  get(index) {
    if (!this.#map.has(index)) return undefined;
    const v = this.#map.get(index);
    this.#map.delete(index);
    this.#map.set(index, v);
    return v;
  }

  set(index, mesh, keep = () => false) {
    this.#map.delete(index);
    this.#map.set(index, mesh);
    this.trim(keep);
  }

  /** Drop the oldest entries `keep` does not name until the spare ones number at most `size`. */
  trim(keep = () => false) {
    let spare = [...this.#map.keys()].filter((k) => !keep(k));
    while (spare.length > this.#size) {
      this.#map.delete(spare.shift());
    }
  }

  clear() {
    this.#map.clear();
  }

  keys() {
    return [...this.#map.keys()];
  }
}

/**
 * Which part numbers need a GPU mesh now (placed ones and the ghost) and which of the
 * ones held may go: { need, drop }. `held` is the set of part numbers with a GPU mesh; spare
 * ones beyond `spare` are dropped, in the order given (oldest first).
 */
export function gpuPlan({ placed, ghost = null, held, spare = GPU_RESIDENT_SPARE }) {
  const need = new Set(placed);
  if (ghost !== null && ghost !== undefined) need.add(ghost);
  const extra = [...held].filter((i) => !need.has(i));
  const drop = extra.length > spare ? extra.slice(0, extra.length - spare) : [];
  return { need, drop };
}
