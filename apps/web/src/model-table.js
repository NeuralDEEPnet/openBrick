// The page's copy of the model on screen (docs/plan/07 sections 2 and 9): the parts the core
// placed, in typed columns indexed by slot, patched with the core's deltas. No DOM and no
// three.js, so node --test checks it. The page only mirrors what the core says here, so it can
// draw, and hand the same table back to a replaced core (src/model-keeper.js): it never decides
// where a part goes (CLAUDE.md: the UI never computes geometry or connectivity).
//
// Ids are unsigned (up to 4,294,967,295): they cross as the bits of an Int32Array and are read with `>>> 0`.
//
// A table of 100,000 parts is about 4 MB (28 bytes a part and the id map), not a list of 100,000
// objects, and an edit changes one slot.

/** Numbers per part in the core's rows: id, catalog part number, x, y, z (LDU), rotation, colour. */
export const ROW = 7;

export class ModelTable {
  #cap;
  #id;
  #part;
  #pos;
  #rot;
  #colour;
  /** id -> slot. */
  #slot = new Map();
  #free = [];
  /** Slots below this have been used. */
  #high = 0;

  constructor(capacity = 256) {
    this.#cap = capacity;
    this.#alloc(capacity);
  }

  #alloc(cap) {
    const grow = (old, Type, per = 1) => {
      const next = new Type(cap * per);
      if (old) next.set(old);
      return next;
    };
    this.#id = grow(this.#id, Uint32Array);
    this.#part = grow(this.#part, Int32Array);
    this.#pos = grow(this.#pos, Int32Array, 3);
    this.#rot = grow(this.#rot, Uint8Array);
    this.#colour = grow(this.#colour, Uint16Array);
    this.#cap = cap;
  }

  /** How many parts are placed. */
  get size() {
    return this.#slot.size;
  }

  has(id) {
    return this.#slot.has(id);
  }

  slotOf(id) {
    return this.#slot.get(id);
  }

  /** Catalog part number, position (x, y, z), rotation and colour of the part in `slot`. */
  partAt(slot) {
    return this.#part[slot];
  }

  posAt(slot, k) {
    return this.#pos[slot * 3 + k];
  }

  rotAt(slot) {
    return this.#rot[slot];
  }

  colourAt(slot) {
    return this.#colour[slot];
  }

  idAt(slot) {
    return this.#id[slot];
  }

  /** The part with this id as { id, part, pos: [x, y, z], rot, colour }, or undefined. */
  get(id) {
    const s = this.#slot.get(id);
    if (s === undefined) return undefined;
    return { id, part: this.#part[s], pos: [this.#pos[s * 3], this.#pos[s * 3 + 1], this.#pos[s * 3 + 2]], rot: this.#rot[s], colour: this.#colour[s] };
  }

  /** Calls `fn(slot)` for every placed part (not in placement order). */
  forEachSlot(fn) {
    for (const slot of this.#slot.values()) fn(slot);
  }

  #put(row, at) {
    // Ids are unsigned 32-bit numbers; the core's rows are Int32Array, so the bits are read back as unsigned.
    const id = row[at] >>> 0;
    let s = this.#slot.get(id);
    if (s === undefined) {
      s = this.#free.length ? this.#free.pop() : this.#high++;
      if (s >= this.#cap) this.#alloc(Math.max(this.#cap * 2, s + 1));
      this.#slot.set(id, s);
      this.#id[s] = id;
    }
    this.#part[s] = row[at + 1];
    this.#pos[s * 3] = row[at + 2];
    this.#pos[s * 3 + 1] = row[at + 3];
    this.#pos[s * 3 + 2] = row[at + 4];
    this.#rot[s] = row[at + 5];
    this.#colour[s] = row[at + 6];
  }

  /** Replace the whole table with the core's rows (the whole table, seven numbers a part). */
  replaceAll(rows) {
    if (rows.length % ROW !== 0) throw new RangeError(`the instance array has ${rows.length} numbers, not a multiple of ${ROW}`);
    this.#slot.clear();
    this.#free.length = 0;
    this.#high = 0;
    const n = rows.length / ROW;
    if (n > this.#cap) this.#alloc(n);
    for (let at = 0; at < rows.length; at += ROW) this.#put(rows, at);
  }

  /**
   * Patch the table with a delta of the core (`Assembler.delta`: a mode number, then rows): mode 1
   * is the whole table, mode 0 lists the parts that changed, a removed part with the part number
   * -1. Returns { full, ids } with the ids that changed (every id, for a whole table).
   */
  applyDelta(delta) {
    const mode = delta[0];
    const rows = delta.subarray ? delta.subarray(1) : delta.slice(1);
    if (rows.length % ROW !== 0) throw new RangeError(`the delta has ${rows.length} numbers, not a multiple of ${ROW}`);
    if (mode === 1) {
      const before = [...this.#slot.keys()];
      this.replaceAll(rows);
      const ids = new Set(before);
      for (let at = 0; at < rows.length; at += ROW) ids.add(rows[at] >>> 0);
      return { full: true, ids: [...ids] };
    }
    if (mode !== 0) throw new RangeError(`unknown delta mode ${mode}`);
    const ids = [];
    for (let at = 0; at < rows.length; at += ROW) {
      const id = rows[at] >>> 0;
      ids.push(id);
      if (rows[at + 1] < 0) {
        const s = this.#slot.get(id);
        if (s !== undefined) {
          this.#slot.delete(id);
          this.#free.push(s);
        }
      } else {
        this.#put(rows, at);
      }
    }
    return { full: false, ids };
  }

  /** The table as the core's rows, in ascending id order (what a replaced core is given back). */
  toRows() {
    const slots = [...this.#slot.entries()].sort((a, b) => a[0] - b[0]);
    const out = new Int32Array(slots.length * ROW);
    let at = 0;
    for (const [id, s] of slots) {
      out[at++] = id;
      out[at++] = this.#part[s];
      out[at++] = this.#pos[s * 3];
      out[at++] = this.#pos[s * 3 + 1];
      out[at++] = this.#pos[s * 3 + 2];
      out[at++] = this.#rot[s];
      out[at++] = this.#colour[s];
    }
    return out;
  }

  /** Parts per catalog part number: Map part -> count. */
  countsByPart() {
    const counts = new Map();
    for (const s of this.#slot.values()) counts.set(this.#part[s], (counts.get(this.#part[s]) ?? 0) + 1);
    return counts;
  }

  /** The catalog ids of the placed parts in ascending id order, for tests; at most `limit`. */
  placed(parts, limit = 2000) {
    if (this.size > limit) return [];
    return [...this.#slot.entries()].sort((a, b) => a[0] - b[0]).map(([, s]) => parts[this.#part[s]].id);
  }
}
