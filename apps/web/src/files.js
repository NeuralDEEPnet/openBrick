// Saving and opening files. In a browser the page saves with a download and
// opens with a file input, as it always has. In the desktop app (apps/desktop,
// a Tauri 2 window around this same page) it calls the app's commands, which
// show the native save and open dialogs and write or read the one file the
// user chose (apps/desktop/src-tauri/src/app.rs). The desktop app is detected
// by `window.__TAURI__`, which only the desktop app defines (its
// tauri.conf.json sets `withGlobalTauri`).
//
// This module only moves bytes; the files come from the core, and the core
// checks what is read (docs/plan/02 section 1).

/** Media types of the files the page saves. */
export const MIME = { "3mf": "model/3mf", stl: "model/stl", json: "application/json" };

/** The desktop app's command names and the headers of a save (src-tauri/src/files.rs). */
export const COMMANDS = { pick: "pick_file", read: "read_picked_file", save: "save_file" };
export const NAME_HEADER = "x-openbrick-name";
export const FORMAT_HEADER = "x-openbrick-format";

/** What the page opens: an SVG drawing or a fit profile (OpenKind in files.rs). */
export const OPEN_KINDS = ["svg", "fit"];

/**
 * The desktop app's bridge, `{ invoke }`, when the page runs inside it (the
 * global Tauri API), else null.
 */
export function desktopBridge(win) {
  const invoke = win?.__TAURI__?.core?.invoke;
  return typeof invoke === "function" ? { invoke } : null;
}

/** A command's raw reply as bytes: an ArrayBuffer over the custom protocol, an array of numbers over the fallback. */
export function replyBytes(reply) {
  if (reply instanceof ArrayBuffer) return new Uint8Array(reply);
  if (ArrayBuffer.isView(reply)) return new Uint8Array(reply.buffer, reply.byteOffset, reply.byteLength);
  if (Array.isArray(reply)) return Uint8Array.from(reply);
  throw new Error("the desktop app answered a read with no file bytes");
}

/**
 * A file the user chose in the desktop app's open dialog, shaped like the
 * browser's `File` as far as the page uses it: `name` and `size` straight
 * away, and the bytes read only when asked, at most `maxBytes`. The page
 * checks `size` against the core's limit first, so a file that is too large
 * is never read.
 */
export function pickedFile(bridge, kind, picked, maxBytes) {
  const bytes = async () => replyBytes(await bridge.invoke(COMMANDS.read, { kind, maxBytes }));
  return {
    name: String(picked.name),
    size: Number(picked.size),
    async arrayBuffer() {
      const b = await bytes();
      return b.buffer.slice(b.byteOffset, b.byteOffset + b.byteLength);
    },
    async text() {
      return new TextDecoder().decode(await bytes());
    },
  };
}

/**
 * Hand bytes to the browser as a download, named `name`. `doc` and `urls`
 * default to the page's document and `URL`; tests pass stand-ins.
 */
export function browserDownload(bytes, name, format, { doc = globalThis.document, urls = globalThis.URL, later = setTimeout } = {}) {
  const url = urls.createObjectURL(new Blob([bytes], { type: MIME[format] ?? "application/octet-stream" }));
  const a = doc.createElement("a");
  a.href = url;
  a.download = name;
  a.hidden = true;
  doc.body.append(a);
  a.click();
  a.remove();
  later(() => urls.revokeObjectURL(url), 60_000);
}

/**
 * The status line after a save (`save` below): "Saved <name>" and the size in
 * a browser, the full path in the desktop app, or a note that nothing was
 * saved when the user closed the dialog.
 */
export function savedText(saved, sizeText) {
  if (!saved.saved) return [`Not saved: the save dialog was closed without choosing a file.`];
  return [`Saved ${saved.path ?? saved.name}`, sizeText];
}

/** The text of an error: the desktop app's commands reject with plain strings. */
export function errorText(err) {
  if (typeof err === "string") return err;
  return err?.message ?? String(err);
}

/**
 * The page's file access.
 *
 * - `desktop`: true inside the desktop app.
 * - `save(bytes, name, format)`: resolves to `{ saved, name, path }`. In a
 *   browser the download always starts (`saved: true`, `path: null`); in the
 *   desktop app `saved` is false when the user cancelled the dialog, and
 *   `name` and `path` are the file the user chose.
 * - `open(kind, maxBytes)` (desktop only): the chosen file (see
 *   `pickedFile`), or null when the user cancelled. In a browser the page
 *   keeps its file inputs, which `open` does not replace.
 */
export function createFileAccess({ bridge = null, download = browserDownload } = {}) {
  if (!bridge) {
    return {
      desktop: false,
      async save(bytes, name, format) {
        download(bytes, name, format);
        return { saved: true, name, path: null };
      },
      open: null,
    };
  }
  return {
    desktop: true,
    async save(bytes, name, format) {
      if (!(format in MIME)) throw new Error(`cannot save a ${format} file`);
      const reply = await bridge.invoke(COMMANDS.save, bytes, {
        headers: { [NAME_HEADER]: encodeURIComponent(name), [FORMAT_HEADER]: format },
      });
      if (!reply) return { saved: false, name, path: null };
      return { saved: true, name: reply.name, path: reply.path };
    },
    async open(kind, maxBytes) {
      if (!OPEN_KINDS.includes(kind)) throw new Error(`cannot open a ${kind} file`);
      const picked = await bridge.invoke(COMMANDS.pick, { kind });
      return picked ? pickedFile(bridge, kind, picked, maxBytes) : null;
    },
  };
}
