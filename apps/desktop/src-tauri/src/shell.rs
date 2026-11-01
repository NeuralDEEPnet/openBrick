//! Window rules of the desktop app that hold no Tauri types: which links leave
//! the app for the system browser, and the Linux "safe rendering" switch
//! (docs/plan/09 section 3).

use std::ffi::OsStr;

/// The web places the page links to: the repository (source, the print test
/// guide, the notice of the Neuraldeep.net marks) and the published
/// playground. Each is `(host, path prefix)`; only `https` links qualify.
const EXTERNAL_LINKS: &[(&str, &str)] = &[
    ("github.com", "/NeuralDEEPnet/openBrick"),
    ("neuraldeepnet.github.io", "/openBrick"),
];

/// Whether a link the user clicked opens in the system browser. The app's
/// window itself never leaves the app's own pages; every other link is
/// ignored. `path` is the URL's path after parsing, so `..` is already gone.
pub fn opens_in_browser(scheme: &str, host: Option<&str>, path: &str) -> bool {
    scheme == "https"
        && EXTERNAL_LINKS.iter().any(|(h, prefix)| {
            host == Some(*h)
                && path
                    .strip_prefix(prefix)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with('/'))
        })
}

/// The command-line switch for safe rendering.
pub const SAFE_RENDERING_FLAG: &str = "--safe-rendering";
/// The environment variable for safe rendering (`1` turns it on).
pub const SAFE_RENDERING_ENV: &str = "OPENBRICK_SAFE_RENDERING";

/// The WebKitGTK variables safe rendering sets before the webview starts: no
/// DMA-BUF renderer, the documented cure for a blank or flickering window on
/// some NVIDIA setups (docs/plan/09 section 3). It stays off unless asked for,
/// because it is no general cure: under Xvfb, WebKitGTK 2.52 with it leaves
/// the 3D view empty while its defaults draw WebGL. Not set either:
/// `WEBKIT_DISABLE_COMPOSITING_MODE`, which turns off the accelerated
/// compositing that WebKit needs to show WebGL at all (the same Xvfb run
/// leaves the 3D view empty with it).
pub const SAFE_RENDERING_VARS: &[(&str, &str)] = &[("WEBKIT_DISABLE_DMABUF_RENDERER", "1")];

/// Whether safe rendering is asked for, by the flag among `args` or by
/// [`SAFE_RENDERING_ENV`] set to `1`.
pub fn safe_rendering_requested<I, S>(args: I, env: Option<&OsStr>) -> bool
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    env.is_some_and(|v| v == "1")
        || args
            .into_iter()
            .any(|a| a.as_ref() == OsStr::new(SAFE_RENDERING_FLAG))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_project_links_open_in_the_browser() {
        let gh = Some("github.com");
        assert!(opens_in_browser("https", gh, "/NeuralDEEPnet/openBrick"));
        assert!(opens_in_browser(
            "https",
            gh,
            "/NeuralDEEPnet/openBrick/blob/main/parts/assets/brand/NOTICE.md"
        ));
        assert!(opens_in_browser(
            "https",
            gh,
            "/NeuralDEEPnet/openBrick/blob/main/docs/guides/print-test.md"
        ));
        assert!(opens_in_browser(
            "https",
            Some("neuraldeepnet.github.io"),
            "/openBrick/"
        ));
        // Another repository, another host, plain HTTP, a look-alike host or path.
        assert!(!opens_in_browser("https", gh, "/NeuralDEEPnet/other"));
        assert!(!opens_in_browser(
            "https",
            gh,
            "/NeuralDEEPnet/openBrick-fork"
        ));
        assert!(!opens_in_browser("https", gh, "/"));
        assert!(!opens_in_browser("http", gh, "/NeuralDEEPnet/openBrick"));
        assert!(!opens_in_browser(
            "https",
            Some("github.com.example.org"),
            "/NeuralDEEPnet/openBrick"
        ));
        assert!(!opens_in_browser("https", None, "/NeuralDEEPnet/openBrick"));
        assert!(!opens_in_browser("file", None, "/etc/passwd"));
        assert!(!opens_in_browser("javascript", None, "alert(1)"));
    }

    #[test]
    fn safe_rendering_is_off_unless_asked_for() {
        let none: [&str; 0] = [];
        assert!(!safe_rendering_requested(none, None));
        assert!(!safe_rendering_requested(["openbrick-desktop"], None));
        assert!(!safe_rendering_requested(
            ["openbrick-desktop"],
            Some(OsStr::new("0"))
        ));
        assert!(safe_rendering_requested(
            ["openbrick-desktop", "--safe-rendering"],
            None
        ));
        assert!(safe_rendering_requested(
            ["openbrick-desktop"],
            Some(OsStr::new("1"))
        ));
    }
}
