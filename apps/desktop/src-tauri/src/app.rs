//! The Tauri application: the main window and the three commands the page's
//! desktop adapter (apps/web/src/files.js) calls.
//!
//! - `pick_file(kind)` shows the native open dialog for an SVG drawing, a
//!   fit profile, a part document, a model or an LDraw part pack and answers with the chosen file's name and size, or `null`
//!   when the user cancelled. The path stays here.
//! - `read_picked_file(kind, maxBytes)` reads that file once, at most
//!   `maxBytes` (and never more than the kind's [`OpenKind::max_read_bytes`]), and answers with its
//!   bytes.
//! - `save_file(bytes)` takes the file bytes as the request body and the
//!   suggested name and format in two headers, shows the native save dialog,
//!   and writes the bytes to the path the user chose. It answers with the
//!   saved file's name and path, or `null` when the user cancelled. The body
//!   is raw bytes over Tauri's custom IPC protocol and a JSON array of numbers
//!   over its postMessage fallback ([`save_bytes`]).
//!
//! The webview gets no dialog, file system, shell or opener permission
//! (capabilities/main.json grants only these three commands), so the page can
//! read only a file the user just chose and write only where the user said.

use std::borrow::Cow;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::Serialize;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::webview::NewWindowResponse;
use tauri::{Manager, State, Url, WebviewWindow, WebviewWindowBuilder};
use tauri_plugin_dialog::DialogExt;

use crate::files::{self, OpenKind, SaveFormat, FORMAT_HEADER, NAME_HEADER};
use crate::shell;

/// Debug builds only: when this variable names a folder, the dialogs are not
/// shown. A save writes `<folder>/<suggested name>`; an open picks
/// `<folder>/open.svg`, `<folder>/open.json` (a fit profile) or
/// `<folder>/open.obpart.json` (a part) by kind (`OpenKind::test_file_name`;
/// a cancel when it is missing).
/// The desktop smoke test (apps/desktop/smoke.mjs) drives the app this way,
/// because WebDriver cannot reach a native dialog. Release builds do not
/// contain this code.
pub const TEST_DIALOGS_ENV: &str = "OPENBRICK_DESKTOP_TEST_DIALOGS";

/// The label of the main window in tauri.conf.json.
const MAIN_WINDOW: &str = "main";

/// The file the user chose last with `pick_file`, until it is read.
#[derive(Default)]
struct Picked(Mutex<Option<(OpenKind, PathBuf)>>);

/// The folder of the last file opened or saved, where the next dialog starts.
#[derive(Default)]
struct LastFolder(Mutex<Option<PathBuf>>);

impl LastFolder {
    fn get(&self) -> Option<PathBuf> {
        self.0.lock().ok().and_then(|f| f.clone())
    }

    fn remember(&self, file: &Path) {
        if let (Some(dir), Ok(mut last)) = (file.parent(), self.0.lock()) {
            *last = Some(dir.to_path_buf());
        }
    }
}

/// The answer to `pick_file`.
#[derive(Serialize)]
struct PickedFile {
    name: String,
    size: u64,
}

/// The answer to `save_file`.
#[derive(Serialize)]
struct SavedFile {
    name: String,
    path: String,
}

#[cfg(debug_assertions)]
fn scripted_dialogs() -> Option<PathBuf> {
    std::env::var_os(TEST_DIALOGS_ENV)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

#[cfg(not(debug_assertions))]
fn scripted_dialogs() -> Option<PathBuf> {
    None
}

/// The file bytes of a save, from the request body.
///
/// Over Tauri's custom IPC protocol the page's `Uint8Array` arrives as the raw
/// body. When the webview cannot use that protocol (Tauri then warns "IPC
/// custom protocol failed" and switches the page to `postMessage` for good;
/// Android always does so), the page's IPC script turns the `Uint8Array` into
/// a JSON array of numbers (`Array.from`, tauri 2.12.5
/// `scripts/process-ipc-message-fn.js`) and Tauri hands it over as
/// `InvokeBody::Json` (`src/ipc/protocol.rs`). The two headers travel in
/// `options.headers` either way. Every item must be a whole number from 0 to
/// 255; anything else is refused, never truncated.
fn save_bytes(body: &InvokeBody) -> Result<Cow<'_, [u8]>, String> {
    match body {
        InvokeBody::Raw(bytes) => Ok(Cow::Borrowed(bytes)),
        InvokeBody::Json(serde_json::Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(i, item)| {
                item.as_u64()
                    .and_then(|n| u8::try_from(n).ok())
                    .ok_or_else(|| {
                        format!(
                            "the save's bytes are broken: item {i} is {item}, not a whole number from 0 to 255"
                        )
                    })
            })
            .collect::<Result<Vec<u8>, String>>()
            .map(Cow::Owned),
        InvokeBody::Json(_) => Err("the save carries no file bytes".to_string()),
    }
}

fn poisoned<T>(_: T) -> String {
    "the desktop app's state is broken after an earlier error; restart the app".to_string()
}

#[tauri::command]
async fn pick_file(
    window: WebviewWindow,
    picked: State<'_, Picked>,
    folder: State<'_, LastFolder>,
    kind: String,
) -> Result<Option<PickedFile>, String> {
    let kind = OpenKind::parse(&kind)?;
    let chosen = match scripted_dialogs() {
        Some(dir) => Some(dir.join(kind.test_file_name())).filter(|p| p.exists()),
        None => {
            let mut dialog = window
                .dialog()
                .file()
                .set_title(kind.dialog_title())
                .add_filter(kind.filter_name(), kind.extensions())
                .set_parent(&window);
            if let Some(dir) = folder.get() {
                dialog = dialog.set_directory(dir);
            }
            dialog
                .blocking_pick_file()
                .map(|p| p.into_path())
                .transpose()
                .map_err(|e| format!("the chosen file has no usable path: {e}"))?
        }
    };
    let mut slot = picked.0.lock().map_err(poisoned)?;
    *slot = None;
    let Some(path) = chosen else {
        return Ok(None);
    };
    let meta =
        std::fs::metadata(&path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if !meta.is_file() {
        return Err(format!(
            "{} is not a regular file (a folder, device or pipe); choose a file",
            path.display()
        ));
    }
    folder.remember(&path);
    let answer = PickedFile {
        name: files::display_name(&path),
        size: meta.len(),
    };
    *slot = Some((kind, path));
    Ok(Some(answer))
}

#[tauri::command]
async fn read_picked_file(
    picked: State<'_, Picked>,
    kind: String,
    max_bytes: u64,
) -> Result<Response, String> {
    let kind = OpenKind::parse(&kind)?;
    // Taken, so the file is read at most once per choice.
    let taken = picked.0.lock().map_err(poisoned)?.take();
    let Some((_, path)) = taken.filter(|(k, _)| *k == kind) else {
        return Err("no file is chosen for this; choose the file again".to_string());
    };
    let bytes = files::read_limited(&path, max_bytes.min(kind.max_read_bytes()))?;
    Ok(Response::new(bytes))
}

#[tauri::command]
async fn save_file(
    window: WebviewWindow,
    folder: State<'_, LastFolder>,
    request: Request<'_>,
) -> Result<Option<SavedFile>, String> {
    let bytes = save_bytes(request.body())?;
    let header = |name: &str| {
        request
            .headers()
            .get(name)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| format!("the save lacks its {name} header"))
    };
    let format = SaveFormat::parse(header(FORMAT_HEADER)?)?;
    let suggested = files::suggested_file_name(&files::decode_name(header(NAME_HEADER)?)?, format);
    let chosen = match scripted_dialogs() {
        Some(dir) => Some(dir.join(&suggested)),
        None => {
            let mut dialog = window
                .dialog()
                .file()
                .set_title(format.dialog_title())
                .add_filter(format.filter_name(), &[format.extension()])
                .set_file_name(&suggested)
                .set_parent(&window);
            // Like a browser download, the first save starts in Downloads.
            if let Some(dir) = folder.get().or_else(|| window.path().download_dir().ok()) {
                dialog = dialog.set_directory(dir);
            }
            dialog
                .blocking_save_file()
                .map(|p| p.into_path())
                .transpose()
                .map_err(|e| format!("the chosen place has no usable path: {e}"))?
        }
    };
    let Some(chosen) = chosen else {
        return Ok(None);
    };
    let (path, extension_added) = files::with_format_extension(&chosen, format);
    files::check_save_target(&path, extension_added)?;
    files::write_atomically(&path, &bytes)?;
    folder.remember(&path);
    Ok(Some(SavedFile {
        name: files::display_name(&path),
        path: path.display().to_string(),
    }))
}

/// Whether `url` is one of the app's own pages: the bundled frontend
/// (`tauri://localhost`, or `http://tauri.localhost` on Windows) or, in
/// `tauri dev`, the dev server.
fn is_app_page(url: &Url, dev_url: Option<&Url>) -> bool {
    match url.scheme() {
        "tauri" => url.host_str() == Some("localhost"),
        "http" | "https" => {
            url.host_str() == Some("tauri.localhost")
                || dev_url.is_some_and(|dev| dev.origin() == url.origin())
        }
        _ => false,
    }
}

/// Open one of the project's links in the system browser; ignore any other.
fn open_outside(url: &Url) {
    if shell::opens_in_browser(url.scheme(), url.host_str(), url.path()) {
        let target = url.to_string();
        std::thread::spawn(move || {
            if let Err(e) = opener::open_browser(&target) {
                eprintln!("openbrick-desktop: could not open {target} in the browser: {e}");
            }
        });
    } else {
        eprintln!("openbrick-desktop: ignored a link to {url}");
    }
}

/// Start the app. Never returns while the window is open.
pub fn run() {
    // Linux safe rendering (docs/plan/09 section 3): WebKitGTK reads these
    // variables when the webview starts, so they are set first.
    #[cfg(target_os = "linux")]
    if shell::safe_rendering_requested(
        std::env::args_os().skip(1),
        std::env::var_os(shell::SAFE_RENDERING_ENV).as_deref(),
    ) {
        for (key, value) in shell::SAFE_RENDERING_VARS {
            std::env::set_var(key, value);
        }
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(Picked::default())
        .manage(LastFolder::default())
        .invoke_handler(tauri::generate_handler![
            pick_file,
            read_picked_file,
            save_file
        ])
        .setup(|app| {
            let config = app
                .config()
                .app
                .windows
                .iter()
                .find(|w| w.label == MAIN_WINDOW)
                .cloned()
                .ok_or("tauri.conf.json has no main window")?;
            let dev_url = if cfg!(dev) {
                app.config().build.dev_url.clone()
            } else {
                None
            };
            WebviewWindowBuilder::from_config(app.handle(), &config)?
                .on_navigation(move |url| {
                    let inside = is_app_page(url, dev_url.as_ref());
                    if !inside {
                        open_outside(url);
                    }
                    inside
                })
                .on_new_window(|url, _features| {
                    open_outside(&url);
                    NewWindowResponse::Deny
                })
                .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("OpenBrick3D could not start");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn a_save_takes_its_bytes_from_either_ipc_path() {
        // The custom protocol: the raw body, borrowed as it is.
        let raw = InvokeBody::Raw(vec![0, 7, 128, 255]);
        assert_eq!(save_bytes(&raw).unwrap().as_ref(), &[0, 7, 128, 255]);
        assert!(matches!(save_bytes(&raw).unwrap(), Cow::Borrowed(_)));
        // The postMessage fallback: what Array.from(new Uint8Array([0, 7, 128, 255]))
        // serialises to.
        let json = InvokeBody::Json(serde_json::json!([0, 7, 128, 255]));
        assert_eq!(save_bytes(&json).unwrap().as_ref(), &[0, 7, 128, 255]);
        // An empty file is the same on both paths.
        assert!(save_bytes(&InvokeBody::Raw(Vec::new())).unwrap().is_empty());
        assert!(save_bytes(&InvokeBody::Json(serde_json::json!([])))
            .unwrap()
            .is_empty());
    }

    #[test]
    fn a_save_refuses_a_body_that_is_not_bytes() {
        // No payload at all (Tauri's default body is JSON null), or arguments
        // instead of bytes.
        for body in [
            serde_json::json!(null),
            serde_json::json!({ "bytes": [1, 2] }),
        ] {
            let err = save_bytes(&InvokeBody::Json(body)).unwrap_err();
            assert_eq!(err, "the save carries no file bytes");
        }
        // Items outside a byte are refused, never wrapped or truncated:
        // 256 = u8::MAX + 1, -1 and 1.5 are no byte, and neither is a string.
        for (body, bad) in [
            (serde_json::json!([1, 256]), "item 1 is 256"),
            (serde_json::json!([-1]), "item 0 is -1"),
            (serde_json::json!([0, 0, 1.5]), "item 2 is 1.5"),
            (serde_json::json!(["7"]), "item 0 is \"7\""),
        ] {
            let err = save_bytes(&InvokeBody::Json(body)).unwrap_err();
            assert!(err.contains(bad), "{err}");
        }
    }

    #[test]
    fn only_the_app_pages_load_in_the_window() {
        assert!(is_app_page(&url("tauri://localhost/"), None));
        assert!(is_app_page(
            &url("tauri://localhost/index.html?mode=coupon"),
            None
        ));
        assert!(is_app_page(&url("http://tauri.localhost/"), None));
        assert!(!is_app_page(&url("tauri://example.org/"), None));
        assert!(!is_app_page(
            &url("https://github.com/NeuralDEEPnet/openBrick"),
            None
        ));
        assert!(!is_app_page(&url("http://localhost:1420/"), None));
        assert!(!is_app_page(&url("file:///etc/passwd"), None));
        let dev = url("http://localhost:1420");
        assert!(is_app_page(
            &url("http://localhost:1420/?mode=coupon"),
            Some(&dev)
        ));
        assert!(!is_app_page(&url("http://localhost:1421/"), Some(&dev)));
    }
}
