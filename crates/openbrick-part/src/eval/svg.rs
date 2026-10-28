//! SVG artwork as untrusted input: parse an asset with usvg into filled outlines
//! (docs/plan/06 section 5), with every resource limit checked before usvg does
//! any work that could grow with a hostile file.
//!
//! What is read: every visible path with a fill (any paint: colour, gradient or
//! pattern counts as artwork), its `fill-rule`, and its absolute transform (every
//! group, `<use>` and viewBox transform applied by usvg). Strokes are counted and
//! ignored (v1 builds fills; strokes become outlines in v1.1). `<text>`, `<image>`,
//! clip paths, masks and filters are counted and ignored too, so the caller can
//! warn about each.
//!
//! What is never done: no file or network access (the `<image>` resolvers return
//! nothing, so usvg never opens a path or URL), no fonts, no compressed SVGZ.
//!
//! Limits (resource guards, not brick dimensions), each with its own error:
//! [`MAX_SVG_BYTES`], [`MAX_SVG_NODES`] XML nodes, [`MAX_SVG_DEPTH`] levels of
//! element nesting, [`MAX_SVG_ELEMENTS`] elements after `<use>` and other `href`
//! references are expanded, [`MAX_SVG_WORK`] for references to patterns, clip
//! paths, masks and filters, and [`MAX_SVG_PATHS`] filled paths. Entity
//! declarations and `<marker>` elements are refused: both can multiply a small
//! file into an enormous one, and logos do not need them.

use std::collections::HashMap;

use openbrick_geom::region::FillRule;
use usvg::roxmltree;
use usvg::tiny_skia_path::PathSegment;

/// Largest SVG asset, in bytes (4 MiB): far above any logo, small enough that a
/// browser evaluates it without trouble.
pub const MAX_SVG_BYTES: usize = 4 * 1024 * 1024;
/// Most XML nodes (elements, text runs, comments) in one SVG.
pub const MAX_SVG_NODES: u32 = 200_000;
/// Deepest element nesting accepted. usvg converts groups recursively, so this
/// keeps its stack use small, also on wasm32 (whose stack is 1 MiB by default).
pub const MAX_SVG_DEPTH: usize = 64;
/// Most elements after every `<use>` (and other `href="#id"` reference) is
/// expanded: what usvg would build.
pub const MAX_SVG_ELEMENTS: u64 = 100_000;
/// Bound on usvg's work when elements reference patterns, clip paths, masks or
/// filters: expanded elements times (1 + 5 x the largest such definition), since
/// an element can reference at most five of them (fill, stroke, clip-path, mask,
/// filter) and usvg may copy each per element.
pub const MAX_SVG_WORK: u64 = 2_000_000;
/// Most filled paths in one SVG.
pub const MAX_SVG_PATHS: usize = 10_000;

/// One segment of an outline, in SVG canvas units (y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Seg {
    Line([f64; 2]),
    Quad([f64; 2], [f64; 2]),
    Cubic([f64; 2], [f64; 2], [f64; 2]),
}

/// One closed contour (SVG closes open subpaths of a fill).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Subpath {
    pub start: [f64; 2],
    pub segs: Vec<Seg>,
}

/// One filled SVG path: its contours and fill rule.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Shape {
    pub rule: FillRule,
    pub subpaths: Vec<Subpath>,
}

/// What the parser read and what it left out.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Parsed {
    pub shapes: Vec<Shape>,
    /// Paths with a stroke (the stroke is ignored; a fill on the same path is kept).
    pub strokes: usize,
    pub texts: usize,
    pub images: usize,
    /// Groups with a clip path, mask or filter (each applied as if absent).
    pub clip_paths: usize,
    pub masks: usize,
    pub filters: usize,
}

/// Elements whose `href="#id"` makes usvg copy or inherit the target.
const HREF_TAGS: &[&str] = &[
    "use",
    "pattern",
    "linearGradient",
    "radialGradient",
    "filter",
    "feImage",
    "textPath",
    "tref",
];

/// Definitions usvg may convert once per referencing element.
const DEF_TAGS: &[&str] = &["pattern", "clipPath", "mask", "filter"];

fn href_target<'a>(node: roxmltree::Node<'a, '_>) -> Option<&'a str> {
    node.attributes()
        .find(|a| a.name() == "href")
        .and_then(|a| a.value().trim().strip_prefix('#'))
}

/// Checks on the raw XML before usvg runs: nesting depth, markers, and how far
/// references expand. Returns the number of `<text>` and `<image>` elements.
fn prescan(doc: &roxmltree::Document) -> Result<(usize, usize), String> {
    let slots = doc
        .descendants()
        .map(|n| n.id().get_usize())
        .max()
        .unwrap_or(0)
        + 1;
    // Element depth by node id: descendants come in document order, so every
    // parent is seen before its children.
    let mut depth = vec![0usize; slots];
    let mut texts = 0;
    let mut images = 0;
    for n in doc.descendants().filter(|n| n.is_element()) {
        let d = n.parent().map_or(0, |p| depth[p.id().get_usize()]) + 1;
        depth[n.id().get_usize()] = d;
        if d > MAX_SVG_DEPTH {
            return Err(format!(
                "elements are nested more than {MAX_SVG_DEPTH} levels deep; flatten the groups in your editor (ungroup)"
            ));
        }
        match n.tag_name().name() {
            "marker" => {
                return Err(
                    "the SVG uses <marker>, which relief artwork does not support; convert markers to paths in your editor (Inkscape: Path > Object to Path)"
                        .into(),
                )
            }
            "text" => texts += 1,
            "image" => images += 1,
            _ => {}
        }
    }

    // Expanded element counts, children plus href targets, by an explicit-stack
    // depth-first search (reference chains can be long; no recursion).
    let ids: HashMap<&str, roxmltree::NodeId> = doc
        .descendants()
        .filter(|n| n.is_element())
        .filter_map(|n| n.attribute("id").map(|id| (id, n.id())))
        .collect();
    let deps = |n: roxmltree::Node| -> Vec<roxmltree::NodeId> {
        let mut out: Vec<roxmltree::NodeId> = n
            .children()
            .filter(|c| c.is_element())
            .map(|c| c.id())
            .collect();
        if HREF_TAGS.contains(&n.tag_name().name()) {
            if let Some(target) = href_target(n).and_then(|t| ids.get(t)) {
                out.push(*target);
            }
        }
        out
    };
    // 0 = not seen, 1 = on the current path, 2 = done.
    let mut state = vec![0u8; slots];
    let mut size = vec![0u64; slots];
    let root = doc.root_element();
    let mut stack = vec![(root.id(), false)];
    while let Some((id, expanded)) = stack.pop() {
        let Some(node) = doc.get_node(id) else {
            continue;
        };
        let i = id.get_usize();
        if expanded {
            let total = deps(node)
                .iter()
                .fold(1u64, |s, d| s.saturating_add(size[d.get_usize()]));
            if total > MAX_SVG_ELEMENTS {
                return Err(format!(
                    "the SVG expands to more than {MAX_SVG_ELEMENTS} elements through <use> and other references; simplify it or expand the references in your editor"
                ));
            }
            size[i] = total;
            state[i] = 2;
            continue;
        }
        match state[i] {
            2 => continue,
            1 => {
                return Err(
                    "an href reference refers back to itself or to an element that contains it"
                        .into(),
                )
            }
            _ => {}
        }
        state[i] = 1;
        stack.push((id, true));
        for d in deps(node) {
            if state[d.get_usize()] != 2 {
                stack.push((d, false));
            }
        }
    }
    let expanded = size[root.id().get_usize()].max(1);
    let largest_def = doc
        .descendants()
        .filter(|n| n.is_element() && DEF_TAGS.contains(&n.tag_name().name()))
        .map(|n| size[n.id().get_usize()].max(1))
        .max()
        .unwrap_or(0);
    let work = expanded.saturating_mul(largest_def.saturating_mul(5).saturating_add(1));
    if work > MAX_SVG_WORK {
        return Err(format!(
            "the SVG's patterns, clip paths, masks or filters could make usvg build about {work} elements (limit {MAX_SVG_WORK}); simplify them in your editor"
        ));
    }
    Ok((texts, images))
}

/// The usvg options for untrusted artwork: no file or network access for
/// `<image>`, defaults otherwise.
fn options() -> usvg::Options<'static> {
    usvg::Options {
        resources_dir: None,
        image_href_resolver: usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| None),
            resolve_string: Box::new(|_, _| None),
        },
        ..usvg::Options::default()
    }
}

fn finite(p: [f64; 2]) -> Result<[f64; 2], String> {
    if p[0].is_finite() && p[1].is_finite() {
        Ok(p)
    } else {
        Err("a path has a coordinate that is not a finite number".into())
    }
}

/// One usvg path as closed contours in canvas units.
fn outline(path: &usvg::Path) -> Result<Vec<Subpath>, String> {
    let t = path.abs_transform();
    let map = |p: usvg::tiny_skia_path::Point| -> Result<[f64; 2], String> {
        let (x, y) = (f64::from(p.x), f64::from(p.y));
        finite([
            f64::from(t.sx) * x + f64::from(t.kx) * y + f64::from(t.tx),
            f64::from(t.ky) * x + f64::from(t.sy) * y + f64::from(t.ty),
        ])
    };
    let mut out: Vec<Subpath> = Vec::new();
    let mut current: Option<Subpath> = None;
    // Where a segment after `Close` starts when no MoveTo follows.
    let mut last_start = [0.0, 0.0];
    for seg in path.data().segments() {
        let seg = match seg {
            PathSegment::MoveTo(p) => {
                out.extend(current.take());
                let start = map(p)?;
                last_start = start;
                current = Some(Subpath {
                    start,
                    segs: Vec::new(),
                });
                continue;
            }
            PathSegment::Close => {
                out.extend(current.take());
                continue;
            }
            PathSegment::LineTo(p) => Seg::Line(map(p)?),
            PathSegment::QuadTo(a, p) => Seg::Quad(map(a)?, map(p)?),
            PathSegment::CubicTo(a, b, p) => Seg::Cubic(map(a)?, map(b)?, map(p)?),
        };
        current
            .get_or_insert_with(|| Subpath {
                start: last_start,
                segs: Vec::new(),
            })
            .segs
            .push(seg);
    }
    out.extend(current);
    out.retain(|s| !s.segs.is_empty());
    Ok(out)
}

/// Parse SVG bytes into filled shapes. Errors are messages for the user; no
/// input makes this panic (tests/eval_relief.rs runs the hostile fixtures).
pub(crate) fn parse(bytes: &[u8]) -> Result<Parsed, String> {
    if bytes.len() > MAX_SVG_BYTES {
        return Err(format!(
            "the SVG is {} bytes; the limit is {MAX_SVG_BYTES} bytes",
            bytes.len()
        ));
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return Err("compressed SVG (SVGZ) is not accepted; save it as plain SVG".into());
    }
    let text = std::str::from_utf8(bytes)
        .map_err(|_| "the SVG is not UTF-8 text; save it as plain SVG".to_string())?;
    if text.contains("<!ENTITY") {
        return Err(
            "the SVG declares XML entities, which artwork does not accept (they can expand a small file enormously); export it without a DTD"
                .into(),
        );
    }
    let xml = roxmltree::Document::parse_with_options(
        text,
        roxmltree::ParsingOptions {
            // A DOCTYPE line is common in exported SVG; entity declarations were
            // refused above, and external entities are never resolved.
            allow_dtd: true,
            nodes_limit: MAX_SVG_NODES,
            entity_resolver: None,
        },
    )
    .map_err(|e| match e {
        roxmltree::Error::NodesLimitReached => {
            format!("the SVG has more than {MAX_SVG_NODES} XML nodes")
        }
        e => format!("the SVG is not well-formed XML: {e}"),
    })?;
    if xml.root_element().tag_name().name() != "svg" {
        return Err(format!(
            "the file's root element is <{}>, not <svg>",
            xml.root_element().tag_name().name()
        ));
    }
    let (texts, images) = prescan(&xml)?;
    let tree = usvg::Tree::from_xmltree(&xml, &options()).map_err(|e| match e {
        usvg::Error::InvalidSize => {
            "the SVG has no valid size: give the <svg> element a viewBox (or width and height) larger than zero"
                .to_string()
        }
        e => format!("usvg could not read the SVG: {e}"),
    })?;

    let mut parsed = Parsed {
        texts,
        images,
        ..Parsed::default()
    };
    // The order of shapes does not matter: the caller unions them.
    let mut stack: Vec<&usvg::Group> = vec![tree.root()];
    while let Some(group) = stack.pop() {
        parsed.clip_paths += usize::from(group.clip_path().is_some());
        parsed.masks += usize::from(group.mask().is_some());
        parsed.filters += usize::from(!group.filters().is_empty());
        for node in group.children() {
            match node {
                usvg::Node::Group(g) => stack.push(g),
                usvg::Node::Path(p) => {
                    parsed.strokes += usize::from(p.stroke().is_some());
                    let Some(fill) = p.fill() else { continue };
                    if !p.is_visible() || fill.opacity().get() <= 0.0 {
                        continue;
                    }
                    if parsed.shapes.len() >= MAX_SVG_PATHS {
                        return Err(format!(
                            "the SVG has more than {MAX_SVG_PATHS} filled paths; simplify it"
                        ));
                    }
                    let rule = match fill.rule() {
                        usvg::FillRule::NonZero => FillRule::NonZero,
                        usvg::FillRule::EvenOdd => FillRule::EvenOdd,
                    };
                    let subpaths = outline(p)?;
                    if !subpaths.is_empty() {
                        parsed.shapes.push(Shape { rule, subpaths });
                    }
                }
                // Images are never resolved and text is not converted without
                // fonts; both were counted from the XML.
                usvg::Node::Image(_) | usvg::Node::Text(_) => {}
            }
        }
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transforms_are_applied_and_strokes_counted() {
        let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">
            <g transform="translate(2 3)"><rect width="1" height="2"/></g>
            <path d="M0 0 L5 0" stroke="black" fill="none"/>
        </svg>"#;
        let p = parse(svg).expect("parses");
        assert_eq!(p.strokes, 1);
        assert_eq!(p.shapes.len(), 1);
        let s = &p.shapes[0].subpaths[0];
        assert_eq!(s.start, [2.0, 3.0]);
        assert!(s.segs.contains(&Seg::Line([3.0, 5.0])), "{s:?}");
    }

    #[test]
    fn references_are_expanded_and_loops_refused() {
        let bomb = {
            let mut s = String::from(
                r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 1 1"><defs><rect id="a0" width="1" height="1"/>"#,
            );
            for i in 1..12 {
                s.push_str(&format!(r#"<g id="a{i}">"#));
                for _ in 0..10 {
                    s.push_str(&format!(r##"<use xlink:href="#a{}"/>"##, i - 1));
                }
                s.push_str("</g>");
            }
            s.push_str(r##"</defs><use href="#a11"/></svg>"##);
            s
        };
        let e = parse(bomb.as_bytes()).expect_err("a use bomb is refused");
        assert!(e.contains("expands to more than"), "{e}");
        let looped = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1">
            <g id="g"><use href="#g"/></g></svg>"##;
        let e = parse(looped).expect_err("a loop is refused");
        assert!(e.contains("refers back"), "{e}");
    }
}
