//! `cargo xtask docs`: the documentation site (docs/plan/10 section 7, backlog
//! item 16), built with mdBook.
//!
//! 1. Regenerate the CLI reference, `docs/site/src/cli.md`, from the `openbrick`
//!    command's own `--help` output for every command and subcommand (its clap
//!    definitions), so the page always matches the program. With `--check` the
//!    file is compared instead of written, and a stale file fails.
//! 2. Stage the book in `target/docs-src/`: `docs/site/` (book.toml, theme/,
//!    src/) plus the Markdown files of `docs/guides/`, `docs/plan/` and
//!    `docs/spikes/`, read from where they live (never copied into the
//!    repository), as `guides/`, `plan/` and `spikes/`. Links between those
//!    files keep working because the folders keep their places relative to each
//!    other; a link to anything else in the repository (the root README, a
//!    script, a data folder) becomes a link to the file on GitHub. A link to a
//!    file that does not exist fails. Until `docs/guides/install.md` exists,
//!    `docs/site/install-placeholder.md` stands in for it. Every Markdown file
//!    of the staged folders must be in `SUMMARY.md`. Every page gets the same
//!    footer ([`page_footer`]): a link to its source file on GitHub and the
//!    disclaimer from `crates/openbrick-units/disclaimer.txt`, the file the CLI
//!    and the playground read (docs/plan/12 section 4 puts it in the website
//!    footer).
//! 3. Build it with the pinned mdBook ([`MDBOOK_VERSION`]) into `target/docs/`.
//!    Installing mdBook is the developer's job; xtask prints the command.
//! 4. Check every link and anchor in the generated HTML: each relative `href`
//!    or `src` must name a file in the book, and each `#fragment` an `id` in
//!    that file. External links are counted, not fetched. Then check that
//!    every page carries the footer with the disclaimer ([`check_footers`]).
//!
//! `cargo xtask docs --mdbook-version` prints the pinned mdBook version (CI
//! reads it to install and cache the same one). `cargo xtask check` also fails
//! when the committed CLI reference is out of date ([`cli_reference_is_current`]).
//!
//! mdBook (MPL-2.0) runs as a program at build time and is never linked into a
//! crate. The site it writes does contain mdBook's own front-end files,
//! unmodified (scripts, style sheets, page templates, fonts and the libraries
//! it bundles); `docs/site/src/credits.md` names each with its licence.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// The mdBook release the site is built and checked with.
pub const MDBOOK_VERSION: &str = "0.5.4";
/// The committed, generated CLI reference.
pub const CLI_REFERENCE: &str = "docs/site/src/cli.md";
/// Where the book is built.
pub const OUT_DIR: &str = "target/docs";
/// Where the book's sources are staged.
const STAGE_DIR: &str = "target/docs-src";
const SITE_DIR: &str = "docs/site";
const INSTALL_GUIDE: &str = "docs/guides/install.md";
const INSTALL_PLACEHOLDER: &str = "docs/site/install-placeholder.md";
const DISCLAIMER_FILE: &str = "crates/openbrick-units/disclaimer.txt";
/// The credits and licences of what mdBook puts into the site.
const CREDITS_PAGE: &str = "docs/site/src/credits.md";
/// The element every page ends with ([`page_footer`]).
const FOOTER_OPEN: &str = "<footer class=\"ob-page-footer\">";
/// Built pages that are not pages of their own: the whole book on one page
/// (it repeats every chapter's footer; the theme shows the first) and the
/// table of contents for readers without JavaScript.
const NOT_PAGES: &[&str] = &["print.html", "toc.html"];
/// The site's base path on GitHub Pages (`site-url` in docs/site/book.toml).
const SITE_URL: &str = "/openBrick/docs/";
const REPO_URL: &str = "https://github.com/NeuralDEEPnet/openBrick";
/// Folders of the repository the book includes, and their folder in the book.
/// They sit side by side in both places, so relative links between them hold.
const STAGED_FOLDERS: &[(&str, &str)] = &[
    ("docs/guides", "guides"),
    ("docs/plan", "plan"),
    ("docs/spikes", "spikes"),
];
const IMAGE_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "gif", "svg", "webp", "avif"];

/// What `cargo xtask docs` was asked to do.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Options {
    /// Compare the CLI reference instead of writing it.
    pub check: bool,
    /// Only print [`MDBOOK_VERSION`].
    pub mdbook_version: bool,
}

const USAGE: &str = "usage: cargo xtask docs [--check | --mdbook-version]";

impl Options {
    pub fn parse(args: &[String]) -> Result<Options, String> {
        let mut o = Options::default();
        for a in args {
            match a.as_str() {
                "--check" => o.check = true,
                "--mdbook-version" => o.mdbook_version = true,
                other => return Err(format!("unknown option {other}; {USAGE}")),
            }
        }
        Ok(o)
    }
}

/// `cargo xtask docs`.
pub fn docs(root: &Path, options: &Options) -> bool {
    if options.mdbook_version {
        println!("{MDBOOK_VERSION}");
        return true;
    }
    let reference = match cli_reference(root) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("docs: {e}");
            return false;
        }
    };
    let path = root.join(CLI_REFERENCE);
    if options.check {
        if !reference_matches(&path, &reference) {
            return false;
        }
    } else {
        let current = std::fs::read_to_string(&path).ok();
        if current.as_deref() != Some(reference.as_str()) {
            if let Err(e) = std::fs::write(&path, &reference) {
                eprintln!("docs: cannot write {CLI_REFERENCE}: {e}");
                return false;
            }
            println!("docs: wrote {CLI_REFERENCE}");
        }
    }
    let stage = root.join(STAGE_DIR);
    if let Err(errors) = stage_book(root, &stage) {
        for e in errors {
            eprintln!("docs: {e}");
        }
        return false;
    }
    if let Err(e) = credits_name_pinned_mdbook(root) {
        eprintln!("docs: {e}");
        return false;
    }
    if !mdbook_is_pinned() {
        return false;
    }
    let out = root.join(OUT_DIR);
    if out.exists() {
        if let Err(e) = std::fs::remove_dir_all(&out) {
            eprintln!("docs: cannot clear {OUT_DIR}: {e}");
            return false;
        }
    }
    let out_abs = match std::path::absolute(&out) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("docs: {OUT_DIR}: {e}");
            return false;
        }
    };
    let status = Command::new("mdbook")
        .arg("build")
        .arg(&stage)
        .arg("--dest-dir")
        .arg(&out_abs)
        .status();
    match status {
        Ok(s) if s.success() => {}
        Ok(s) => {
            eprintln!("docs: mdbook build failed ({s})");
            return false;
        }
        Err(e) => {
            eprintln!("docs: cannot run mdbook: {e}");
            return false;
        }
    }
    match check_links(&out) {
        Ok(stats) => println!(
            "docs: {} pages, {} internal links and anchors checked, {} external links not fetched",
            stats.pages, stats.internal, stats.external
        ),
        Err(errors) => {
            for e in &errors {
                eprintln!("docs: {e}");
            }
            eprintln!("docs: {} broken links in {OUT_DIR}", errors.len());
            return false;
        }
    }
    let disclaimer = match std::fs::read_to_string(root.join(DISCLAIMER_FILE)) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("docs: cannot read {DISCLAIMER_FILE}: {e}");
            return false;
        }
    };
    match check_footers(&out, &disclaimer) {
        Ok(pages) => {
            println!("docs: {pages} pages end with their source link and the disclaimer");
            println!("docs: built {OUT_DIR}/index.html");
            true
        }
        Err(errors) => {
            for e in &errors {
                eprintln!("docs: {e}");
            }
            eprintln!(
                "docs: {} pages without the footer in {OUT_DIR}",
                errors.len()
            );
            false
        }
    }
}

/// For `cargo xtask check`: the committed CLI reference equals what the CLI
/// prints now. Needs no mdBook.
pub fn cli_reference_is_current(root: &Path) -> bool {
    match cli_reference(root) {
        Ok(reference) => reference_matches(&root.join(CLI_REFERENCE), &reference),
        Err(e) => {
            eprintln!("docs: {e}");
            false
        }
    }
}

fn reference_matches(path: &Path, reference: &str) -> bool {
    // A Windows checkout with core.autocrlf has CRLF line endings; the
    // reference is generated with LF.
    match std::fs::read_to_string(path).map(|t| t.replace("\r\n", "\n")) {
        Ok(current) if current == reference => {
            println!("docs: {CLI_REFERENCE} is up to date");
            true
        }
        Ok(_) => {
            eprintln!(
                "docs: {CLI_REFERENCE} is out of date: the openbrick command's help changed. Run `cargo xtask docs` (or, without mdBook, let it stop after writing the file) and commit the result."
            );
            false
        }
        Err(e) => {
            eprintln!(
                "docs: cannot read {CLI_REFERENCE}: {e}; run `cargo xtask docs` to generate it"
            );
            false
        }
    }
}

// ---------------------------------------------------------------------------
// CLI reference

/// Build the `openbrick` binary and render its help for every command.
pub fn cli_reference(root: &Path) -> Result<String, String> {
    let bin = build_cli(root)?;
    let mut pages = Vec::new();
    collect_help(&bin, &mut Vec::new(), &mut pages)?;
    Ok(render_cli_reference(&pages))
}

/// `cargo build -p openbrick` and the path of the binary it produced (wherever
/// CARGO_TARGET_DIR puts it).
fn build_cli(root: &Path) -> Result<PathBuf, String> {
    let output = Command::new("cargo")
        .current_dir(root)
        .args([
            "build",
            "-q",
            "-p",
            "openbrick",
            "--bin",
            "openbrick",
            "--message-format=json-render-diagnostics",
        ])
        .output()
        .map_err(|e| format!("cannot run cargo: {e}"))?;
    if !output.status.success() {
        return Err(format!(
            "cargo build -p openbrick failed ({})\n{}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|m| m["reason"] == "compiler-artifact" && m["target"]["name"] == "openbrick")
        .find_map(|m| m["executable"].as_str().map(PathBuf::from))
        .ok_or_else(|| "cargo build -p openbrick named no openbrick executable".to_string())
}

/// The help text of `openbrick <path…> --help`, then of each subcommand it lists.
fn collect_help(
    bin: &Path,
    path: &mut Vec<String>,
    pages: &mut Vec<(Vec<String>, String)>,
) -> Result<(), String> {
    let output = Command::new(bin)
        .args(path.iter())
        .arg("--help")
        // Plain text whatever the caller's terminal settings. Without clap's
        // wrap_help feature (not enabled in this workspace) the width is fixed.
        .env_remove("CLICOLOR_FORCE")
        .env_remove("CLICOLOR")
        .env_remove("COLUMNS")
        .env("NO_COLOR", "1")
        .output()
        .map_err(|e| format!("cannot run {}: {e}", bin.display()))?;
    if !output.status.success() {
        let words: Vec<&str> = std::iter::once("openbrick")
            .chain(path.iter().map(String::as_str))
            .chain(std::iter::once("--help"))
            .collect();
        return Err(format!("`{}` failed ({})", words.join(" "), output.status));
    }
    let help = normalise_help(&String::from_utf8_lossy(&output.stdout));
    let subcommands = subcommands(&help);
    pages.push((path.clone(), help));
    for sub in subcommands {
        path.push(sub);
        collect_help(bin, path, pages)?;
        path.pop();
    }
    Ok(())
}

/// Line endings and the Windows binary name as on Linux, no trailing spaces
/// (clap pads blank lines inside long help), one final newline.
fn normalise_help(text: &str) -> String {
    let text = text
        .replace("\r\n", "\n")
        .replace("openbrick.exe", "openbrick");
    let mut out: String = text
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    out.truncate(out.trim_end().len());
    out.push('\n');
    out
}

/// The subcommand names a clap help lists under `Commands:`, without `help`.
fn subcommands(help: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_commands = false;
    for line in help.lines() {
        if line == "Commands:" {
            in_commands = true;
            continue;
        }
        if !in_commands {
            continue;
        }
        if line.is_empty() || !line.starts_with(' ') {
            break;
        }
        // An entry is indented by two spaces; a wrapped description by more.
        if let Some(rest) = line.strip_prefix("  ") {
            if !rest.starts_with(' ') {
                if let Some(name) = rest.split_whitespace().next() {
                    if name != "help" {
                        names.push(name.trim_end_matches(',').to_string());
                    }
                }
            }
        }
    }
    names
}

fn render_cli_reference(pages: &[(Vec<String>, String)]) -> String {
    let title = |path: &[String]| {
        if path.is_empty() {
            "openbrick".to_string()
        } else {
            format!("openbrick {}", path.join(" "))
        }
    };
    let mut out = String::new();
    out.push_str("# CLI reference\n\n");
    out.push_str(
        "<!-- Generated by `cargo xtask docs` from the openbrick command's own --help output (its clap\n     definitions in crates/openbrick/src). Do not edit it by hand: change the doc comments and\n     attributes there and run `cargo xtask docs`. `cargo xtask docs --check` and `cargo xtask check`\n     fail when this file is out of date. -->\n\n",
    );
    out.push_str(
        "The `openbrick` command line builds, validates and calibrates brick-compatible parts. This page is the `--help` text of every command, taken from the program built from this repository, so it matches that version; `openbrick <command> --help` prints the same text. [Install](guides/install.md) describes how to get the command, and the [first print test](guides/print-test.md) walks through it from the calibration coupon to a printed tile.\n\n",
    );
    out.push_str("## Commands\n\n");
    for (path, _) in pages {
        let t = title(path);
        let indent = "  ".repeat(path.len().saturating_sub(1));
        let bullet = if path.is_empty() { "" } else { "  " };
        out.push_str(&format!(
            "{indent}{bullet}- [`{t}`](#{})\n",
            t.replace(' ', "-")
        ));
    }
    for (path, help) in pages {
        out.push_str(&format!("\n## `{}`\n\n```text\n{help}```\n", title(path)));
    }
    out
}

// ---------------------------------------------------------------------------
// Staging

/// Stage the book in `stage`. Returns every problem found.
pub fn stage_book(root: &Path, stage: &Path) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();
    if stage.exists() {
        if let Err(e) = std::fs::remove_dir_all(stage) {
            return Err(vec![format!("cannot clear {}: {e}", stage.display())]);
        }
    }
    let site = root.join(SITE_DIR);
    let src = stage.join("src");
    for (from, to) in [
        (site.join("book.toml"), stage.join("book.toml")),
        (site.join("theme"), stage.join("theme")),
        (site.join("src"), src.clone()),
    ] {
        if let Err(e) = copy_recursive(&from, &to) {
            errors.push(format!("cannot stage {}: {e}", from.display()));
        }
    }
    if !errors.is_empty() {
        return Err(errors);
    }

    // Every page ends with the disclaimer, verbatim from its one source.
    let disclaimer = match std::fs::read_to_string(root.join(DISCLAIMER_FILE)) {
        Ok(d) if !d.trim().is_empty() => Some(d),
        Ok(_) => {
            errors.push(format!("{DISCLAIMER_FILE} is empty"));
            None
        }
        Err(e) => {
            errors.push(format!("cannot read {DISCLAIMER_FILE}: {e}"));
            None
        }
    };
    // The repository file each page of the book comes from (None: no source
    // link, for the page-not-found page).
    let mut sources: BTreeMap<String, Option<String>> = BTreeMap::new();
    for page in markdown_files(&site.join("src")) {
        if page == "SUMMARY.md" {
            continue;
        }
        let source = (page != "404.md").then(|| format!("{SITE_DIR}/src/{page}"));
        sources.insert(page, source);
    }

    // The Markdown files of the staged folders, by repository path.
    let mut book_pages: BTreeMap<String, String> = BTreeMap::new();
    for (repo_dir, book_dir) in STAGED_FOLDERS {
        let dir = root.join(repo_dir);
        let entries = match std::fs::read_dir(&dir) {
            Ok(e) => e,
            Err(e) => {
                errors.push(format!("cannot read {repo_dir}: {e}"));
                continue;
            }
        };
        let mut names: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| e.file_name().into_string().ok())
            .filter(|n| n.ends_with(".md"))
            .collect();
        names.sort();
        for n in names {
            book_pages.insert(format!("{repo_dir}/{n}"), format!("{book_dir}/{n}"));
        }
    }
    let mut placeholder = false;
    if !book_pages.contains_key(INSTALL_GUIDE) {
        // docs/guides/install.md is written by the releases work; until then a
        // short page points to the print-test guide's install section.
        book_pages.insert(INSTALL_GUIDE.to_string(), "guides/install.md".to_string());
        placeholder = true;
    }

    let linker = Linker {
        root,
        stage_src: &src,
        book_pages: &book_pages,
    };
    for (repo_path, book_path) in &book_pages {
        let (source, read_from) = if placeholder && repo_path == INSTALL_GUIDE {
            (INSTALL_PLACEHOLDER, root.join(INSTALL_PLACEHOLDER))
        } else {
            (repo_path.as_str(), root.join(repo_path))
        };
        let text = match std::fs::read_to_string(&read_from) {
            Ok(t) => t,
            Err(e) => {
                errors.push(format!("cannot read {source}: {e}"));
                continue;
            }
        };
        let staged = match rewrite_links(&text, |target| linker.rewrite(repo_path, target)) {
            Ok(t) => t,
            Err(problems) => {
                errors.extend(problems.into_iter().map(|p| format!("{source}: {p}")));
                continue;
            }
        };
        let to = src.join(book_path);
        if let Err(e) = std::fs::create_dir_all(to.parent().expect("a book page has a folder"))
            .and_then(|_| std::fs::write(&to, staged))
        {
            errors.push(format!("cannot stage {source}: {e}"));
        }
        sources.insert(book_path.clone(), Some(source.to_string()));
    }

    // The footer of every page: its source on GitHub and the disclaimer.
    if let Some(disclaimer) = &disclaimer {
        for (book_path, source) in &sources {
            let at = src.join(book_path);
            let footed = std::fs::read_to_string(&at).and_then(|mut text| {
                if !text.ends_with('\n') {
                    text.push('\n');
                }
                text.push_str(&page_footer(source.as_deref(), disclaimer));
                std::fs::write(&at, text)
            });
            if let Err(e) = footed {
                errors.push(format!("cannot add the footer to {book_path}: {e}"));
            }
        }
    }

    // Every page of the staged folders and of docs/site/src is in SUMMARY.md.
    match std::fs::read_to_string(src.join("SUMMARY.md")) {
        Ok(summary) => {
            let listed = summary_links(&summary);
            let mut expected: BTreeSet<String> = book_pages.values().cloned().collect();
            for page in site_pages(&site.join("src")) {
                expected.insert(page);
            }
            for page in &expected {
                if !listed.contains(page) {
                    errors.push(format!(
                        "{page} is not in docs/site/src/SUMMARY.md; add it to the table of contents"
                    ));
                }
            }
            for page in &listed {
                if !expected.contains(page) {
                    errors.push(format!(
                        "docs/site/src/SUMMARY.md lists {page}, which is not a page of the site"
                    ));
                }
            }
        }
        Err(e) => errors.push(format!("cannot read docs/site/src/SUMMARY.md: {e}")),
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The Markdown pages of docs/site/src, as book paths, except SUMMARY.md and
/// the page-not-found page (`input-404` in book.toml), which mdBook renders
/// outside the table of contents.
fn site_pages(dir: &Path) -> Vec<String> {
    markdown_files(dir)
        .into_iter()
        .filter(|p| p != "SUMMARY.md" && p != "404.md")
        .collect()
}

/// Every Markdown file under `dir`, as sorted slash paths relative to it.
fn markdown_files(dir: &Path) -> Vec<String> {
    let mut pages = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in entries.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "md") {
                if let Ok(rel) = p.strip_prefix(dir) {
                    pages.push(slash_path(rel));
                }
            }
        }
    }
    pages.sort();
    pages
}

/// The footer `stage_book` adds to every page: a link to the page's source on
/// GitHub (when it has one) and the disclaimer, whitespace collapsed to one
/// line. It is a raw HTML block (no blank line inside), so mdBook passes it
/// through as written: no smart quotes, no autolinks, the same text on every
/// page, which [`check_footers`] then finds in the built HTML.
pub fn page_footer(source: Option<&str>, disclaimer: &str) -> String {
    let mut f = format!("\n{FOOTER_OPEN}\n");
    if let Some(path) = source {
        let path = escape_html(path);
        f.push_str(&format!(
            "<p class=\"ob-page-source\"><a href=\"{REPO_URL}/blob/main/{path}\">Source of this page on GitHub</a>: <code>{path}</code></p>\n"
        ));
    }
    f.push_str(&format!(
        "<p class=\"ob-disclaimer\">{}</p>\n</footer>\n",
        disclaimer_html(disclaimer)
    ));
    f
}

/// The disclaimer as it appears in every footer.
fn disclaimer_html(disclaimer: &str) -> String {
    escape_html(&disclaimer.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// The chapter paths SUMMARY.md links to.
fn summary_links(summary: &str) -> BTreeSet<String> {
    let mut links = BTreeSet::new();
    let _ = rewrite_links(summary, |target| {
        links.insert(target.to_string());
        Ok(None)
    });
    links
}

struct Linker<'a> {
    root: &'a Path,
    stage_src: &'a Path,
    /// Repository path of every staged page and its path in the book.
    book_pages: &'a BTreeMap<String, String>,
}

impl Linker<'_> {
    /// The link to write in the staged copy of `page` (a repository path) for a
    /// link to `target`, or None to keep it.
    fn rewrite(&self, page: &str, target: &str) -> Result<Option<String>, String> {
        if is_external(target) || target.starts_with('#') || target.is_empty() {
            return Ok(None);
        }
        let (path, fragment) = match target.split_once('#') {
            Some((p, f)) => (p, Some(f)),
            None => (target, None),
        };
        let page_dir = Path::new(page).parent().unwrap_or(Path::new(""));
        let Some(resolved) = normalise(&page_dir.join(path)) else {
            return Err(format!("the link {target} leaves the repository"));
        };
        let resolved = slash_path(&resolved);
        let with_fragment = |base: String| match fragment {
            Some(f) => format!("{base}#{f}"),
            None => base,
        };
        let from_book_dir = self
            .book_pages
            .get(page)
            .and_then(|b| Path::new(b).parent().map(Path::to_path_buf))
            .unwrap_or_default();
        if let Some(book_path) = self.book_pages.get(&resolved) {
            let rel = relative(&from_book_dir, Path::new(book_path));
            return Ok(Some(with_fragment(rel)));
        }
        let on_disk = self.root.join(&resolved);
        if !on_disk.exists() {
            return Err(format!(
                "the link {target} names {resolved}, which does not exist"
            ));
        }
        // An image inside a staged folder is served by the site itself.
        if on_disk.is_file() && is_image(&resolved) {
            if let Some(book_path) = staged_location(&resolved) {
                let to = self.stage_src.join(&book_path);
                let copied = std::fs::create_dir_all(to.parent().expect("a file has a folder"))
                    .and_then(|_| std::fs::copy(&on_disk, &to));
                if let Err(e) = copied {
                    return Err(format!("cannot stage the image {resolved}: {e}"));
                }
                let rel = relative(&from_book_dir, Path::new(&book_path));
                return Ok(Some(with_fragment(rel)));
            }
        }
        let kind = if on_disk.is_dir() { "tree" } else { "blob" };
        Ok(Some(with_fragment(format!(
            "{REPO_URL}/{kind}/main/{resolved}"
        ))))
    }
}

/// The book path of a repository file inside one of the staged folders.
fn staged_location(repo_path: &str) -> Option<String> {
    STAGED_FOLDERS.iter().find_map(|(repo_dir, book_dir)| {
        repo_path
            .strip_prefix(repo_dir)
            .and_then(|rest| rest.strip_prefix('/'))
            .map(|rest| format!("{book_dir}/{rest}"))
    })
}

fn is_image(path: &str) -> bool {
    Path::new(path)
        .extension()
        .and_then(|x| x.to_str())
        .is_some_and(|x| IMAGE_EXTENSIONS.contains(&x.to_ascii_lowercase().as_str()))
}

/// A URL with a scheme (`https:`, `mailto:`) or a protocol-relative one.
fn is_external(target: &str) -> bool {
    if target.starts_with("//") {
        return true;
    }
    match target.find(':') {
        Some(i) => {
            let scheme = &target[..i];
            !scheme.is_empty()
                && scheme
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        }
        None => false,
    }
}

/// `a/b/../c` as `a/c`; None if the path climbs above its start.
fn normalise(path: &Path) -> Option<PathBuf> {
    let mut out = PathBuf::new();
    for c in path.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                if !out.pop() {
                    return None;
                }
            }
            Component::Normal(p) => out.push(p),
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    Some(out)
}

/// The relative link from folder `from` to file `to`, both book paths.
fn relative(from: &Path, to: &Path) -> String {
    let from: Vec<_> = from.components().collect();
    let to_parts: Vec<_> = to.components().collect();
    let common = from
        .iter()
        .zip(&to_parts)
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> = vec!["..".to_string(); from.len() - common];
    parts.extend(
        to_parts[common..]
            .iter()
            .map(|c| c.as_os_str().to_string_lossy().into_owned()),
    );
    parts.join("/")
}

fn slash_path(p: &Path) -> String {
    p.components()
        .map(|c| c.as_os_str().to_string_lossy().into_owned())
        .collect::<Vec<_>>()
        .join("/")
}

fn copy_recursive(from: &Path, to: &Path) -> std::io::Result<()> {
    if from.is_dir() {
        std::fs::create_dir_all(to)?;
        for e in std::fs::read_dir(from)? {
            let e = e?;
            copy_recursive(&e.path(), &to.join(e.file_name()))?;
        }
        Ok(())
    } else {
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::copy(from, to).map(|_| ())
    }
}

/// Apply `f` to the target of every inline link (`[text](target "title")`)
/// and link reference definition (`[label]: target`) outside code, and write
/// the text back with the targets `f` replaces. Fenced code blocks and code
/// spans are copied as they are. Every error `f` returns is collected.
pub fn rewrite_links(
    text: &str,
    mut f: impl FnMut(&str) -> Result<Option<String>, String>,
) -> Result<String, Vec<String>> {
    let mut out = String::with_capacity(text.len());
    let mut errors = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut apply = |target: &str, out: &mut String| match f(target) {
        Ok(Some(new)) => out.push_str(&new),
        Ok(None) => out.push_str(target),
        Err(e) => {
            errors.push(e);
            out.push_str(target);
        }
    };
    for line in text.split_inclusive('\n') {
        let body = line.trim_end_matches(['\n', '\r']);
        let ending = &line[body.len()..];
        let indent = body.len() - body.trim_start_matches(' ').len();
        let trimmed = &body[indent..];
        if let Some((c, n)) = fence {
            let run = trimmed.chars().take_while(|&x| x == c).count();
            if indent < 4 && run >= n && trimmed[run * c.len_utf8()..].trim().is_empty() {
                fence = None;
            }
            out.push_str(line);
            continue;
        }
        if indent < 4 {
            for c in ['`', '~'] {
                let run = trimmed.chars().take_while(|&x| x == c).count();
                if run >= 3 && !(c == '`' && trimmed[run..].contains('`')) {
                    fence = Some((c, run));
                }
            }
            if fence.is_some() {
                out.push_str(line);
                continue;
            }
        }
        // A link reference definition: `[label]: target`, the label being the
        // first bracketed text on the line.
        if indent < 4 && trimmed.starts_with('[') && !trimmed.starts_with("[^") {
            let close = trimmed.find(']').unwrap_or(0);
            if close > 1 && trimmed[close + 1..].starts_with(':') {
                let after = &trimmed[close + 2..];
                let lead = after.len() - after.trim_start().len();
                let rest = &after[lead..];
                let (target, tail) = split_target(rest);
                if !target.is_empty() {
                    let target_start =
                        indent + close + 2 + lead + usize::from(rest.starts_with('<'));
                    out.push_str(&body[..target_start]);
                    apply(target, &mut out);
                    out.push_str(tail);
                    out.push_str(ending);
                    continue;
                }
            }
        }
        // Inline links, skipping code spans.
        let bytes = body.as_bytes();
        let mut i = 0;
        let mut copied = 0;
        while i < bytes.len() {
            if bytes[i] == b'`' {
                let run = bytes[i..].iter().take_while(|&&b| b == b'`').count();
                let delimiter = &body[i..i + run];
                match body[i + run..].find(delimiter) {
                    Some(end) => i += run + end + run,
                    None => i += run,
                }
                continue;
            }
            if bytes[i] == b']' && bytes.get(i + 1) == Some(&b'(') {
                let start = i + 2;
                let (target, _) = split_target(&body[start..]);
                if !target.is_empty() {
                    let target_start = start + usize::from(body[start..].starts_with('<'));
                    out.push_str(&body[copied..target_start]);
                    apply(target, &mut out);
                    copied = target_start + target.len();
                    i = copied;
                    continue;
                }
            }
            i += 1;
        }
        out.push_str(&body[copied..]);
        out.push_str(ending);
    }
    if errors.is_empty() {
        Ok(out)
    } else {
        Err(errors)
    }
}

/// The link destination at the start of `s` and what follows it: `<…>`, or
/// up to white space or the `)` that closes the link (parentheses balance).
fn split_target(s: &str) -> (&str, &str) {
    if let Some(rest) = s.strip_prefix('<') {
        return match rest.find('>') {
            Some(end) => (&rest[..end], &rest[end..]),
            None => ("", s),
        };
    }
    let mut depth = 0usize;
    for (i, c) in s.char_indices() {
        match c {
            '(' => depth += 1,
            ')' if depth == 0 => return (&s[..i], &s[i..]),
            ')' => depth -= 1,
            c if c.is_whitespace() => return (&s[..i], &s[i..]),
            _ => {}
        }
    }
    (s, "")
}

// ---------------------------------------------------------------------------
// mdBook

/// The site's credits page names mdBook's front-end files and links to their
/// source at the pinned release (MPL-2.0 section 3.1). A new pin can bring
/// other files or licences, so the page must be read again whenever
/// [`MDBOOK_VERSION`] changes; this fails until its links name that release.
fn credits_name_pinned_mdbook(root: &Path) -> Result<(), String> {
    let text = std::fs::read_to_string(root.join(CREDITS_PAGE))
        .map_err(|e| format!("cannot read {CREDITS_PAGE}: {e}"))?;
    if credits_name_version(&text, MDBOOK_VERSION) {
        Ok(())
    } else {
        Err(format!(
            "{CREDITS_PAGE} does not link to mdBook's front-end source and licence at v{MDBOOK_VERSION}, the pinned version: compare the files in {OUT_DIR}/ with the page, update it, and change its links to v{MDBOOK_VERSION}"
        ))
    }
}

fn credits_name_version(text: &str, version: &str) -> bool {
    let repo = "https://github.com/rust-lang/mdBook";
    text.contains(&format!(
        "{repo}/tree/v{version}/crates/mdbook-html/front-end"
    )) && text.contains(&format!("{repo}/blob/v{version}/LICENSE"))
}

fn mdbook_is_pinned() -> bool {
    let install = format!("cargo install mdbook --locked --version {MDBOOK_VERSION}");
    match Command::new("mdbook").arg("--version").output() {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let found = text
                .split_whitespace()
                .last()
                .unwrap_or("")
                .trim_start_matches('v')
                .to_string();
            if found == MDBOOK_VERSION {
                true
            } else {
                eprintln!(
                    "docs: mdbook {found} is installed, but the site is built and checked with mdbook {MDBOOK_VERSION} (book.toml uses its options). Install that version:\n    {install} --force"
                );
                false
            }
        }
        _ => {
            eprintln!(
                "docs: mdbook was not found on the PATH. Install the pinned version once (it compiles in a minute or two):\n    {install}\nThe CLI reference ({CLI_REFERENCE}) is already up to date; only the book build needs mdbook."
            );
            false
        }
    }
}

// ---------------------------------------------------------------------------
// Link check of the built book

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LinkStats {
    pub pages: usize,
    pub internal: usize,
    pub external: usize,
}

/// The HTML files under `out`, sorted.
fn html_files(out: &Path) -> Result<Vec<PathBuf>, Vec<String>> {
    let mut files = Vec::new();
    let mut stack = vec![out.to_path_buf()];
    while let Some(d) = stack.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| vec![format!("{}: {e}", d.display())])?;
        for e in entries.filter_map(|e| e.ok()) {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "html") {
                files.push(p);
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Every built page under `out` (except [`NOT_PAGES`]) ends with exactly one
/// page footer, and that footer carries `disclaimer` (the text of
/// `crates/openbrick-units/disclaimer.txt`) as [`page_footer`] writes it.
/// Returns the number of pages checked.
pub fn check_footers(out: &Path, disclaimer: &str) -> Result<usize, Vec<String>> {
    let expected = format!(
        "<p class=\"ob-disclaimer\">{}</p>",
        disclaimer_html(disclaimer)
    );
    let mut errors = Vec::new();
    let mut pages = 0;
    for file in html_files(out)? {
        let shown = file.strip_prefix(out).map(slash_path).unwrap_or_default();
        if NOT_PAGES.contains(&shown.as_str()) {
            continue;
        }
        pages += 1;
        let text = std::fs::read_to_string(&file).map_err(|e| vec![format!("{shown}: {e}")])?;
        let footers = text.matches(FOOTER_OPEN).count();
        if footers != 1 {
            errors.push(format!(
                "{shown}: {footers} page footers, expected 1 (stage_book adds one to every page)"
            ));
            continue;
        }
        let footer = &text[text.find(FOOTER_OPEN).unwrap_or(0)..];
        let footer = &footer[..footer.find("</footer>").unwrap_or(footer.len())];
        if !footer.contains(&expected) {
            errors.push(format!(
                "{shown}: the page footer does not carry the disclaimer of {DISCLAIMER_FILE}"
            ));
        }
    }
    if errors.is_empty() {
        Ok(pages)
    } else {
        Err(errors)
    }
}

/// Check every relative link and fragment in the HTML files under `out`.
pub fn check_links(out: &Path) -> Result<LinkStats, Vec<String>> {
    let files = html_files(out)?;
    let mut ids: BTreeMap<PathBuf, BTreeSet<String>> = BTreeMap::new();
    let mut texts = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).map_err(|e| vec![format!("{}: {e}", f.display())])?;
        let mut set = BTreeSet::new();
        for attr in ["id", "name"] {
            for v in attribute_values(&text, attr) {
                set.insert(decode_entities(&v));
            }
        }
        ids.insert(f.clone(), set);
        texts.push((f.clone(), text));
    }
    let mut stats = LinkStats {
        pages: files.len(),
        ..LinkStats::default()
    };
    let mut errors = Vec::new();
    for (file, text) in &texts {
        let shown = file.strip_prefix(out).map(slash_path).unwrap_or_default();
        // The 404 page sets <base href="/openBrick/docs/">: its relative links
        // resolve from the site's root, wherever the page is served.
        let base = text.find("<base ").and_then(|at| {
            let tag = &text[at..at + text[at..].find('>').unwrap_or(0)];
            attribute_values(tag, "href").into_iter().next()
        });
        let from_dir = match base.as_deref() {
            Some(SITE_URL) => out.to_path_buf(),
            Some(other) => {
                errors.push(format!(
                    "{shown}: <base href=\"{other}\"> is not the site's path {SITE_URL}"
                ));
                continue;
            }
            None => file.parent().unwrap_or(out).to_path_buf(),
        };
        for attr in ["href", "src"] {
            for raw in attribute_values(text, attr) {
                let url = decode_entities(&raw);
                if is_external(&url) {
                    stats.external += 1;
                    continue;
                }
                if base.as_deref() == Some(url.as_str()) {
                    // The <base> element itself.
                    continue;
                }
                stats.internal += 1;
                let (path, fragment) = match url.split_once('#') {
                    Some((p, f)) => (p, Some(f)),
                    None => (url.as_str(), None),
                };
                let path = path.split('?').next().unwrap_or("");
                let target = if path.is_empty() {
                    file.clone()
                } else {
                    let decoded = percent_decode(path);
                    let joined = if let Some(abs) = decoded.strip_prefix(SITE_URL) {
                        out.join(abs)
                    } else if decoded.starts_with('/') {
                        errors.push(format!(
                            "{shown}: {url} is an absolute path outside the site ({SITE_URL})"
                        ));
                        continue;
                    } else {
                        from_dir.join(&decoded)
                    };
                    let Some(rel) = joined.strip_prefix(out).ok().and_then(normalise) else {
                        errors.push(format!("{shown}: {url} points outside the book"));
                        continue;
                    };
                    let mut t = out.join(rel);
                    if decoded.ends_with('/') || t.is_dir() {
                        t = t.join("index.html");
                    }
                    t
                };
                if !target.is_file() {
                    errors.push(format!("{shown}: {url}: no such file in the book"));
                    continue;
                }
                if let Some(fragment) = fragment.filter(|f| !f.is_empty()) {
                    let fragment = percent_decode(fragment);
                    match ids.get(&target) {
                        Some(set) if set.contains(&fragment) => {}
                        Some(_) => errors.push(format!(
                            "{shown}: {url}: {} has no element with id \"{fragment}\"",
                            target.strip_prefix(out).map(slash_path).unwrap_or_default()
                        )),
                        // A fragment on a non-HTML file (a script, a font).
                        None => {}
                    }
                }
            }
        }
    }
    if errors.is_empty() {
        Ok(stats)
    } else {
        Err(errors)
    }
}

/// The values of every `name="…"` attribute in `html` (double-quoted, as
/// mdBook writes them), preceded by white space so `data-id` is not `id`.
fn attribute_values(html: &str, name: &str) -> Vec<String> {
    let needle = format!("{name}=\"");
    let mut values = Vec::new();
    let mut from = 0;
    while let Some(pos) = html[from..].find(&needle) {
        let at = from + pos;
        let start = at + needle.len();
        let preceded = html[..at]
            .chars()
            .next_back()
            .is_some_and(char::is_whitespace);
        match html[start..].find('"') {
            Some(len) => {
                if preceded {
                    values.push(html[start..start + len].to_string());
                }
                from = start + len + 1;
            }
            None => break,
        }
    }
    values
}

fn decode_entities(s: &str) -> String {
    s.replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&amp;", "&")
}

/// `%20` and friends as bytes; invalid escapes stay as they are.
fn percent_decode(s: &str) -> String {
    let hex = |b: u8| char::from(b).to_digit(16);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                // Two hex digits make one byte: h and l are below 16.
                out.push((h * 16 + l) as u8);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "openbrick-xtask-docs-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn options_parse() {
        assert_eq!(Options::parse(&[]).unwrap(), Options::default());
        assert!(Options::parse(&["--check".into()]).unwrap().check);
        assert!(
            Options::parse(&["--mdbook-version".into()])
                .unwrap()
                .mdbook_version
        );
        assert!(Options::parse(&["--chek".into()]).is_err());
    }

    #[test]
    fn subcommands_come_from_the_commands_section() {
        let help = "About\n\nUsage: openbrick <COMMAND>\n\nCommands:\n  standard   Print the standard\n  calibrate  Print the coupon and record\n             the cells (wrapped)\n  help       Print this message\n\nOptions:\n  -h, --help  Print help\n";
        assert_eq!(subcommands(help), ["standard", "calibrate"]);
        assert!(subcommands("Usage: openbrick about\n\nOptions:\n  -h, --help\n").is_empty());
    }

    #[test]
    fn help_is_normalised() {
        assert_eq!(
            normalise_help("Usage: openbrick.exe build  \r\n          \r\nx\n\n\n"),
            "Usage: openbrick build\n\nx\n"
        );
    }

    #[test]
    fn the_reference_links_every_command_to_its_section() {
        let pages = vec![
            (vec![], "top\n".to_string()),
            (vec!["calibrate".to_string()], "cal\n".to_string()),
            (
                vec!["calibrate".to_string(), "coupon".to_string()],
                "coupon\n".to_string(),
            ),
        ];
        let md = render_cli_reference(&pages);
        assert!(md.contains("- [`openbrick`](#openbrick)\n"));
        assert!(md.contains("  - [`openbrick calibrate`](#openbrick-calibrate)\n"));
        assert!(md.contains("    - [`openbrick calibrate coupon`](#openbrick-calibrate-coupon)\n"));
        assert!(md.contains("\n## `openbrick calibrate coupon`\n\n```text\ncoupon\n```\n"));
    }

    #[test]
    fn links_are_rewritten_outside_code_only() {
        let text = "See [a](a.md#x) and [b](<b c.md> \"t\") and `[c](c.md)`.\n```sh\n[d](d.md)\n```\n[ref]: e.md\n~~~\n[f](f.md)\n~~~\n[g](h(1).md)\n";
        let mut seen = Vec::new();
        let out = rewrite_links(text, |t| {
            seen.push(t.to_string());
            Ok(Some(format!("X{t}")))
        })
        .unwrap();
        assert_eq!(seen, ["a.md#x", "b c.md", "e.md", "h(1).md"]);
        assert_eq!(
            out,
            "See [a](Xa.md#x) and [b](<Xb c.md> \"t\") and `[c](c.md)`.\n```sh\n[d](d.md)\n```\n[ref]: Xe.md\n~~~\n[f](f.md)\n~~~\n[g](Xh(1).md)\n"
        );
        let err = rewrite_links("[a](a.md) [b](b.md)\n", |t| Err(format!("bad {t}"))).unwrap_err();
        assert_eq!(err, ["bad a.md", "bad b.md"]);
    }

    #[test]
    fn staged_links_stay_relative_or_go_to_github() {
        let root = temp("linker");
        for f in [
            "docs/plan/a.md",
            "docs/spikes/s.md",
            "scripts/x.py",
            "docs/plan/img/p.png",
        ] {
            let p = root.join(f);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, "x").unwrap();
        }
        std::fs::create_dir_all(root.join("docs/plan/research")).unwrap();
        let pages: BTreeMap<String, String> = [
            ("docs/plan/a.md", "plan/a.md"),
            ("docs/spikes/s.md", "spikes/s.md"),
        ]
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect();
        let stage = root.join("stage");
        let l = Linker {
            root: &root,
            stage_src: &stage,
            book_pages: &pages,
        };
        assert_eq!(
            l.rewrite("docs/plan/a.md", "https://x.org/a.md").unwrap(),
            None
        );
        assert_eq!(l.rewrite("docs/plan/a.md", "#top").unwrap(), None);
        assert_eq!(
            l.rewrite("docs/spikes/s.md", "../plan/a.md#2-x").unwrap(),
            Some("../plan/a.md#2-x".into())
        );
        assert_eq!(
            l.rewrite("docs/spikes/s.md", "../../scripts/x.py").unwrap(),
            Some(format!("{REPO_URL}/blob/main/scripts/x.py"))
        );
        assert_eq!(
            l.rewrite("docs/plan/a.md", "research/").unwrap(),
            Some(format!("{REPO_URL}/tree/main/docs/plan/research"))
        );
        assert_eq!(
            l.rewrite("docs/plan/a.md", "img/p.png").unwrap(),
            Some("img/p.png".into())
        );
        assert!(stage.join("plan/img/p.png").is_file());
        assert!(l.rewrite("docs/plan/a.md", "missing.md").is_err());
        assert!(l.rewrite("docs/plan/a.md", "../../../outside.md").is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn relative_paths_and_schemes() {
        assert_eq!(
            relative(Path::new("spikes"), Path::new("plan/a.md")),
            "../plan/a.md"
        );
        assert_eq!(relative(Path::new("plan"), Path::new("plan/a.md")), "a.md");
        assert_eq!(relative(Path::new(""), Path::new("plan/a.md")), "plan/a.md");
        assert!(is_external("https://a"));
        assert!(is_external("mailto:a@b"));
        assert!(is_external("//cdn"));
        assert!(!is_external("a.md"));
        assert!(!is_external("a.md#x:y"));
        assert_eq!(percent_decode("a%20b%2"), "a b%2");
        assert_eq!(percent_decode("%e2%80%94"), "\u{2014}");
    }

    #[test]
    fn the_link_check_finds_missing_files_and_anchors() {
        let out = temp("links");
        std::fs::create_dir_all(out.join("plan")).unwrap();
        std::fs::write(
            out.join("index.html"),
            r##"<a href="plan/a.html#ok">a</a> <a href="https://x.org">x</a> <img src="i.png"> <a href="#top">t</a> <h1 id="top">T</h1>"##,
        )
        .unwrap();
        std::fs::write(out.join("i.png"), "").unwrap();
        std::fs::write(
            out.join("plan/a.html"),
            r##"<h2 id="ok">ok</h2><a href="../index.html#top">u</a><a data-href="nope.html">d</a>"##,
        )
        .unwrap();
        std::fs::write(
            out.join("404.html"),
            r##"<base href="/openBrick/docs/"><a href="plan/a.html">a</a>"##,
        )
        .unwrap();
        let stats = check_links(&out).unwrap();
        assert_eq!(
            stats,
            LinkStats {
                pages: 3,
                internal: 5,
                external: 1
            }
        );
        std::fs::write(
            out.join("plan/b.html"),
            r##"<a href="a.html#gone">g</a><a href="../missing.html">m</a><a href="../../up.html">u</a>"##,
        )
        .unwrap();
        let errors = check_links(&out).unwrap_err();
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(
            errors[0].contains("no element with id \"gone\""),
            "{errors:?}"
        );
        assert!(errors[1].contains("no such file"), "{errors:?}");
        assert!(errors[2].contains("outside the book"), "{errors:?}");
        let _ = std::fs::remove_dir_all(&out);
    }

    /// A small repository with everything `stage_book` reads.
    fn site_tree(name: &str) -> PathBuf {
        let root = temp(name);
        let files: &[(&str, &str)] = &[
            ("docs/site/book.toml", "[book]\n"),
            ("docs/site/theme/openbrick.css", "a {}\n"),
            (
                "docs/site/src/SUMMARY.md",
                "# Summary\n\n[Home](index.md)\n\n- [Install](guides/install.md)\n- [Plan](plan/README.md)\n- [S](spikes/s.md)\n",
            ),
            ("docs/site/src/index.md", "# Home\n\n[Plan](plan/README.md)"),
            ("docs/site/src/404.md", "# Not found\n"),
            ("docs/site/install-placeholder.md", "# Install (soon)\n"),
            (
                "docs/plan/README.md",
                "# Plan\n\n[s](../spikes/s.md#x) [script](../../scripts/x.py)\n",
            ),
            ("docs/spikes/s.md", "# S\n\n```sh\nopen\n```\n"),
            ("scripts/x.py", "x\n"),
            (
                "crates/openbrick-units/disclaimer.txt",
                "Not affiliated with\n  anyone & everyone <here>.\n",
            ),
        ];
        for (path, text) in files {
            let p = root.join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        std::fs::create_dir_all(root.join("docs/guides")).unwrap();
        root
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
    }

    #[test]
    fn stage_book_stages_every_page_with_its_footer() {
        let root = site_tree("stage-ok");
        let stage = root.join("target/docs-src");
        stage_book(&root, &stage).unwrap();
        let src = stage.join("src");
        let disclaimer = "<p class=\"ob-disclaimer\">Not affiliated with anyone &amp; everyone &lt;here&gt;.</p>";
        let source = |path: &str| format!("<a href=\"{REPO_URL}/blob/main/{path}\">");

        // The install placeholder stands in for the missing guide.
        let install = read(&src.join("guides/install.md"));
        assert!(install.starts_with("# Install (soon)\n\n<footer class=\"ob-page-footer\">\n"));
        assert!(install.contains(&source("docs/site/install-placeholder.md")));
        // Links between staged folders stay relative; others go to GitHub.
        let plan = read(&src.join("plan/README.md"));
        assert!(plan.contains("[s](../spikes/s.md#x)"), "{plan}");
        assert!(plan.contains(&format!("[script]({REPO_URL}/blob/main/scripts/x.py)")));
        assert!(plan.contains(&source("docs/plan/README.md")));
        // A page without a final newline still gets its footer on lines of
        // its own, after a blank line.
        let index = read(&src.join("index.md"));
        assert!(
            index.contains("[Plan](plan/README.md)\n\n<footer"),
            "{index}"
        );
        assert!(index.contains(&source("docs/site/src/index.md")));
        // The page-not-found page has the disclaimer but no source link.
        let not_found = read(&src.join("404.md"));
        assert!(!not_found.contains("ob-page-source"));
        // Every page except SUMMARY.md ends with the footer, once.
        for page in [
            "index.md",
            "404.md",
            "guides/install.md",
            "plan/README.md",
            "spikes/s.md",
        ] {
            let text = read(&src.join(page));
            assert_eq!(text.matches(FOOTER_OPEN).count(), 1, "{page}");
            assert!(
                text.ends_with(&format!("{disclaimer}\n</footer>\n")),
                "{page}: {text}"
            );
        }
        assert!(!read(&src.join("SUMMARY.md")).contains(FOOTER_OPEN));
        assert!(stage.join("book.toml").is_file() && stage.join("theme/openbrick.css").is_file());

        // Once the guide exists it replaces the placeholder.
        std::fs::write(root.join("docs/guides/install.md"), "# Install\n").unwrap();
        stage_book(&root, &stage).unwrap();
        let install = read(&src.join("guides/install.md"));
        assert!(install.starts_with("# Install\n\n<footer"));
        assert!(install.contains(&source("docs/guides/install.md")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn stage_book_fails_on_summary_gaps_broken_links_and_no_disclaimer() {
        let root = site_tree("stage-bad");
        let stage = root.join("target/docs-src");
        // A page that is not in SUMMARY.md, and one SUMMARY.md lists but
        // nobody wrote.
        std::fs::write(root.join("docs/plan/extra.md"), "# Extra\n").unwrap();
        let summary = root.join("docs/site/src/SUMMARY.md");
        let mut text = read(&summary);
        text.push_str("- [Gone](plan/gone.md)\n");
        std::fs::write(&summary, text).unwrap();
        let errors = stage_book(&root, &stage).unwrap_err();
        assert_eq!(errors.len(), 2, "{errors:?}");
        assert!(
            errors[0].contains("plan/extra.md is not in docs/site/src/SUMMARY.md"),
            "{errors:?}"
        );
        assert!(
            errors[1].contains("lists plan/gone.md, which is not a page"),
            "{errors:?}"
        );
        std::fs::remove_file(root.join("docs/plan/extra.md")).unwrap();
        std::fs::write(
            &summary,
            read(&summary).replace("- [Gone](plan/gone.md)\n", ""),
        )
        .unwrap();
        stage_book(&root, &stage).unwrap();

        // A link to a file that does not exist.
        std::fs::write(root.join("docs/spikes/s.md"), "[x](../plan/missing.md)\n").unwrap();
        let errors = stage_book(&root, &stage).unwrap_err();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(
            errors[0].starts_with(
                "docs/spikes/s.md: the link ../plan/missing.md names docs/plan/missing.md"
            ),
            "{errors:?}"
        );
        std::fs::write(root.join("docs/spikes/s.md"), "# S\n").unwrap();

        // No disclaimer, no site.
        let disclaimer = root.join(DISCLAIMER_FILE);
        std::fs::write(&disclaimer, " \n").unwrap();
        let errors = stage_book(&root, &stage).unwrap_err();
        assert_eq!(errors, [format!("{DISCLAIMER_FILE} is empty")]);
        std::fs::remove_file(&disclaimer).unwrap();
        let errors = stage_book(&root, &stage).unwrap_err();
        assert!(
            errors[0].starts_with(&format!("cannot read {DISCLAIMER_FILE}")),
            "{errors:?}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_footer_check_finds_pages_without_the_disclaimer() {
        let out = temp("footers");
        let disclaimer = "Not affiliated  with\nanyone.\n";
        let page = |source| format!("<main><h1>T</h1>{}</main>", page_footer(source, disclaimer));
        std::fs::create_dir_all(out.join("plan")).unwrap();
        std::fs::write(out.join("index.html"), page(Some("docs/site/src/index.md"))).unwrap();
        std::fs::write(out.join("404.html"), page(None)).unwrap();
        std::fs::write(out.join("plan/a.html"), page(Some("docs/plan/a.md"))).unwrap();
        // print.html repeats every footer and toc.html has none: both skipped.
        std::fs::write(out.join("print.html"), page(None).repeat(3)).unwrap();
        std::fs::write(out.join("toc.html"), "<ol></ol>").unwrap();
        assert_eq!(check_footers(&out, disclaimer), Ok(3));

        // A page without a footer, one with two, and one whose disclaimer
        // differs from the file (or was rendered as Markdown and changed).
        std::fs::write(out.join("plan/b.html"), "<main><h1>B</h1></main>").unwrap();
        std::fs::write(out.join("plan/c.html"), page(None).repeat(2)).unwrap();
        std::fs::write(
            out.join("plan/d.html"),
            page(None).replace("anyone.", "anyone at all."),
        )
        .unwrap();
        let errors = check_footers(&out, disclaimer).unwrap_err();
        assert_eq!(errors.len(), 3, "{errors:?}");
        assert!(
            errors[0].starts_with("plan/b.html: 0 page footers"),
            "{errors:?}"
        );
        assert!(
            errors[1].starts_with("plan/c.html: 2 page footers"),
            "{errors:?}"
        );
        assert!(
            errors[2].starts_with("plan/d.html: the page footer does not carry the disclaimer"),
            "{errors:?}"
        );
        let _ = std::fs::remove_dir_all(&out);
    }

    #[test]
    fn the_credits_must_name_the_pinned_mdbook() {
        let page = "[src](https://github.com/rust-lang/mdBook/tree/v0.5.4/crates/mdbook-html/front-end) [licence](https://github.com/rust-lang/mdBook/blob/v0.5.4/LICENSE)";
        assert!(credits_name_version(page, "0.5.4"));
        assert!(!credits_name_version(page, "0.5.5"));
        assert!(!credits_name_version(
            &page[..page.find(" [licence]").unwrap()],
            "0.5.4"
        ));
        // The committed page names the version xtask pins.
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        credits_name_pinned_mdbook(&root).unwrap();
    }

    #[test]
    fn summary_links_are_its_chapters() {
        let s = "# Summary\n\n[Home](index.md)\n\n# Part\n\n- [A](plan/a.md)\n  - [B](plan/b.md)\n";
        let links = summary_links(s);
        assert_eq!(
            links.into_iter().collect::<Vec<_>>(),
            ["index.md", "plan/a.md", "plan/b.md"]
        );
    }
}
