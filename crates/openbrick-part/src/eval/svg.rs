//! SVG artwork as untrusted input: parse an asset with usvg into filled outlines
//! (docs/plan/06 section 5), with every resource limit checked before usvg (or
//! roxmltree, or the CSS parser) does any work that could grow with a hostile
//! file.
//!
//! What is read: every visible path with a fill (any paint: colour, gradient or
//! pattern counts as artwork) whose opacity, its groups' included, is above zero;
//! its `fill-rule`; and its absolute transform (every group, `<use>` and viewBox
//! transform applied by usvg). Strokes are counted and ignored (v1 builds fills;
//! strokes become outlines in v1.1). `<text>`, `<image>`, clip paths, masks and
//! filters are counted and ignored too, and so is the number of different fill
//! paints, so the caller can warn about each.
//!
//! What is never done: no file or network access (the `<image>` resolvers return
//! nothing, so usvg never opens a path or URL), no fonts, no compressed SVGZ.
//!
//! Limits (resource guards, not brick dimensions), each with its own error, and
//! why each exists (usvg 0.49.0, roxmltree 0.21.2, simplecss 0.2.2):
//!
//! - [`MAX_SVG_BYTES`] for the file and [`MAX_SVG_NODES`] XML nodes.
//! - [`MAX_SVG_ATTRIBUTES`] per element: roxmltree compares each attribute with
//!   the earlier ones of its element, so one element with 40,000 attributes takes
//!   2.4 s to parse, and the time grows with their square.
//! - `<!ENTITY>` declarations and `<marker>` elements are refused: both can
//!   multiply a small file into an enormous one, and logos do not need them.
//! - CSS in `<style>`: at most [`MAX_SVG_CSS_DECLARATIONS`] declarations once
//!   selector lists are expanded (simplecss copies a block's declarations to
//!   every selector of its list, so `a,a,a,...{...}` multiplies memory), at most
//!   [`MAX_CSS_SELECTOR_PARTS`] parts per selector, and [`MAX_SVG_CSS_WORK`] for
//!   selector matching. usvg matches every rule against every element, and a
//!   descendant selector backtracks through the ancestors: on a 30-deep file of
//!   1.1 KB, `x g g g g g g g g` takes 44 s, `x g g g g g g` 3.7 s.
//! - [`MAX_SVG_DEPTH`] levels of nesting and references together: usvg follows
//!   nesting, `<use>`, `href` and every `url(#id)` (clip paths, masks, filters,
//!   patterns, gradients) by recursion, so a long chain of any of them overflows
//!   the stack, which aborts the process (a chain of 1000 clip paths, 84 KB, on
//!   a 1 MiB stack). Reference cycles are refused.
//! - [`MAX_SVG_ELEMENTS`] elements once every `<use>` is expanded (usvg's tree),
//!   and [`MAX_SVG_WORK`] for building that tree and converting the definitions
//!   that elements reference (attributes copied, clip paths, masks, filters and
//!   paint servers converted per reference).
//! - [`MAX_SVG_PATHS`] filled paths.
//!
//! Every limit is checked by iteration on the raw XML, never by recursion, and
//! before the library whose work it bounds runs.

use std::collections::{BTreeSet, HashMap};

use openbrick_geom::region::FillRule;
use usvg::roxmltree;
use usvg::tiny_skia_path::PathSegment;

/// Largest SVG asset, in bytes (4 MiB): far above any logo, small enough that a
/// browser evaluates it without trouble.
pub const MAX_SVG_BYTES: usize = 4 * 1024 * 1024;
/// Most XML nodes (elements, text runs, comments) in one SVG.
pub const MAX_SVG_NODES: u32 = 200_000;
/// Most attributes on one element (see the module comment: roxmltree's check
/// for duplicates is quadratic in this number). Exported logos use a handful.
pub const MAX_SVG_ATTRIBUTES: usize = 1_000;
/// Longest chain of element nesting and references (`<use>`, `href`, and
/// `url(#id)` in clip-path, mask, filter, fill and stroke), counted in elements
/// along the chain. usvg follows each link by recursion. Measured with usvg
/// 0.49.0 on x86-64, one level takes at most 3.7 KB of stack in a release build
/// and 9.2 KB in a debug build (the worst kinds: a `<use>` linked straight to
/// another `<use>`, and a pattern whose shape is filled with the next pattern),
/// so 64 levels need under 0.6 MiB even in a debug build, inside the 1 MiB stack
/// of wasm32. tests/eval_relief.rs builds a chain of every kind at this limit on
/// a 1 MiB thread. Exported logos stay far below it.
pub const MAX_SVG_DEPTH: usize = 64;
/// Most elements once every `<use>` is expanded: the size of usvg's own tree.
/// usvg expands each `<use>` wherever it is, inside `<defs>` too, so a chain of
/// n groups each using the next one expands to about n^2 elements.
pub const MAX_SVG_ELEMENTS: u64 = 100_000;
/// Bound on usvg's work: each element of its tree counts 1 plus its attributes,
/// and each reference to a clip path, mask, filter, pattern or gradient counts
/// the work of converting that definition again (once per referencing element,
/// and once per element below it for the inherited `fill` and `stroke`), since
/// usvg converts definitions in object-bounding-box units per element.
pub const MAX_SVG_WORK: u64 = 2_000_000;
/// Most CSS declarations in `<style>` once every selector list is expanded.
pub const MAX_SVG_CSS_DECLARATIONS: u64 = 100_000;
/// Most compound selectors in one CSS selector (`g > .a + rect` has three).
/// simplecss matches them by recursion, one level per part.
pub const MAX_CSS_SELECTOR_PARTS: usize = 16;
/// Bound on CSS selector matching: for every element of usvg's tree and every
/// rule, the rule's parts times the deepest nesting to the power of its
/// descendant combinators (the backtracking bound), plus its declarations.
pub const MAX_SVG_CSS_WORK: u64 = 10_000_000;
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
    /// Different fill paints of the shapes built: colours by value, gradients
    /// and patterns by id. More than one usually means artwork that relies on
    /// colour or paint order, such as white shapes painted over dark ones.
    pub fill_paints: usize,
}

/// Elements whose `href="#id"` usvg follows: `<use>` copies its target into
/// usvg's tree; the others are read while converting.
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

/// Properties whose `url(#id)` is inherited, so every element below the one
/// that sets it may follow it.
const INHERITED_URL_PROPERTIES: &[&str] = &[
    "fill",
    "stroke",
    "marker",
    "marker-start",
    "marker-mid",
    "marker-end",
];

/// Calls `f` with every id that `value` refers to with `url(#id)`, read
/// generously so no reference usvg follows is missed: svgtypes reads
/// `url('#a b')` as `a b` and `url(#a b)` as `a`, so both readings are given.
fn url_ids<'a>(value: &'a str, mut f: impl FnMut(&'a str)) {
    let stop = |c: char| c == ')' || c == '\'' || c == '"';
    let mut rest = value;
    while let Some(at) = rest.find("url(") {
        rest = &rest[at + 4..];
        let s = rest.trim_start();
        let s = s.strip_prefix(['\'', '"']).unwrap_or(s).trim_start();
        let Some(s) = s.strip_prefix('#') else {
            continue;
        };
        let to_quote = s.find(stop).map_or(s, |i| &s[..i]);
        f(to_quote.trim_end());
        f(to_quote
            .find(char::is_whitespace)
            .map_or(to_quote, |i| &to_quote[..i]));
    }
}

/// The ids an `href` names (`#id`, read both ways as in [`url_ids`]).
fn href_ids<'a>(value: &'a str, mut f: impl FnMut(&'a str)) {
    if let Some(s) = value.trim().strip_prefix('#') {
        f(s);
        f(s.find(char::is_whitespace).map_or(s, |i| &s[..i]));
    }
}

/// Refuses an element with more than [`MAX_SVG_ATTRIBUTES`] attributes, by a
/// linear scan of the raw text before roxmltree parses it: every `=` outside
/// quotes inside a start tag counts (comments, CDATA, processing instructions
/// and declarations are skipped). In a well-formed file that is the attribute
/// count; in a malformed one it can only count more.
fn check_attribute_counts(text: &str) -> Result<(), String> {
    let refuse = || {
        Err(format!(
            "an element has more than {MAX_SVG_ATTRIBUTES} attributes; simplify the SVG"
        ))
    };
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'<' {
            i += 1;
            continue;
        }
        let rest = &text[i..];
        let skip_to = |open: usize, close: &str| {
            rest[open..]
                .find(close)
                .map_or(b.len(), |j| i + open + j + close.len())
        };
        if rest.starts_with("<!--") {
            i = skip_to(4, "-->");
        } else if rest.starts_with("<![CDATA[") {
            i = skip_to(9, "]]>");
        } else if rest.starts_with("<?") {
            i = skip_to(2, "?>");
        } else if rest.starts_with("<!") {
            i = skip_to(2, ">");
        } else {
            // A tag: count `=` outside quoted values until its `>`.
            let mut count = 0usize;
            let mut quote = None;
            i += 1;
            while i < b.len() {
                let c = b[i];
                i += 1;
                match quote {
                    Some(q) if c == q => quote = None,
                    Some(_) => {}
                    None => match c {
                        b'"' | b'\'' => quote = Some(c),
                        b'=' => {
                            count += 1;
                            if count > MAX_SVG_ATTRIBUTES {
                                return refuse();
                            }
                        }
                        b'>' => break,
                        _ => {}
                    },
                }
            }
        }
    }
    Ok(())
}

/// An upper bound on the declarations simplecss stores for `css`: each block's
/// declarations (at most its colons) times the selectors before it (at most its
/// commas plus one). Comments and strings can only make it larger.
fn css_declaration_bound(css: &str) -> u64 {
    let (mut total, mut selectors, mut colons, mut depth) = (0u64, 1u64, 0u64, 0u32);
    for c in css.bytes() {
        match c {
            b'{' => {
                depth += 1;
                if depth == 1 {
                    colons = 0;
                }
            }
            b'}' if depth > 0 => {
                depth -= 1;
                if depth == 0 {
                    total = total.saturating_add(selectors.saturating_mul(colons.max(1)));
                    selectors = 1;
                }
            }
            b',' if depth == 0 => selectors += 1,
            b':' if depth > 0 => colons += 1,
            _ => {}
        }
    }
    if depth > 0 {
        // A block left open runs to the end of the text.
        total = total.saturating_add(selectors.saturating_mul(colons.max(1)));
    }
    total
}

/// A roxmltree element as simplecss sees it, matched exactly as usvg 0.49.0
/// matches it (`XmlNode` in usvg's svgtree/parse.rs), so the same rules apply.
#[derive(Clone, Copy)]
struct CssNode<'a, 'input>(roxmltree::Node<'a, 'input>);

impl simplecss::Element for CssNode<'_, '_> {
    fn parent_element(&self) -> Option<Self> {
        self.0.parent_element().map(CssNode)
    }

    fn prev_sibling_element(&self) -> Option<Self> {
        self.0.prev_sibling_element().map(CssNode)
    }

    fn has_local_name(&self, name: &str) -> bool {
        self.0.tag_name().name() == name
    }

    fn attribute_matches(&self, name: &str, operator: simplecss::AttributeOperator) -> bool {
        self.0.attribute(name).is_some_and(|v| operator.matches(v))
    }

    fn pseudo_class_matches(&self, class: simplecss::PseudoClass) -> bool {
        matches!(class, simplecss::PseudoClass::FirstChild) && self.prev_sibling_element().is_none()
    }
}

/// Parts and descendant combinators of a selector, read from its text form
/// (parts joined by " ", " > " or " + "). Spaces inside attribute selectors can
/// only make either count larger.
fn selector_shape(selector: &simplecss::Selector) -> (usize, u32) {
    let text = selector.to_string();
    let (mut parts, mut others) = (0usize, 0usize);
    for token in text.split(' ').filter(|t| !t.is_empty()) {
        if token == ">" || token == "+" {
            others += 1;
        } else {
            parts += 1;
        }
    }
    let descendant = parts.saturating_sub(1).saturating_sub(others);
    (parts, u32::try_from(descendant).unwrap_or(u32::MAX))
}

/// What one element makes usvg follow.
#[derive(Clone, Debug, Default)]
struct Links {
    /// `<use>` targets: usvg copies them into its tree.
    copies: Vec<roxmltree::NodeId>,
    /// Followed while converting this element: clip-path, mask, filter, and the
    /// `href` of patterns, gradients, filters and feImage.
    direct: Vec<roxmltree::NodeId>,
    /// Inherited paint references (fill, stroke): every element below may
    /// follow them.
    inherited: Vec<roxmltree::NodeId>,
}

/// Adds the elements `value` refers to with `url(#id)` to `l`, as inherited
/// or direct references by `property`.
fn add_url(
    ids: &HashMap<&str, Vec<roxmltree::NodeId>>,
    l: &mut Links,
    property: &str,
    value: &str,
) {
    let inherited = INHERITED_URL_PROPERTIES.contains(&property);
    url_ids(value, |id| {
        for &t in ids.get(id).into_iter().flatten() {
            if inherited {
                l.inherited.push(t);
            } else {
                l.direct.push(t);
            }
        }
    });
}

/// The elements usvg puts under `n` in its own tree: the child elements, and
/// the targets of a `<use>`.
fn structural<'a>(
    n: roxmltree::Node<'a, '_>,
    links: &'a [Links],
) -> impl Iterator<Item = roxmltree::NodeId> + 'a {
    n.children()
        .filter(|c| c.is_element())
        .map(|c| c.id())
        .chain(links[n.id().get_usize()].copies.iter().copied())
}

/// What the prescan counted in the raw XML.
struct Census {
    texts: usize,
    images: usize,
}

/// Checks on the raw XML before usvg runs (see the module comment for each
/// limit): nesting depth, markers, CSS, and how far references reach and
/// expand.
fn prescan(doc: &roxmltree::Document) -> Result<Census, String> {
    let slots = doc
        .descendants()
        .map(|n| n.id().get_usize())
        .max()
        .unwrap_or(0)
        + 1;
    // Element depth by node id: descendants come in document order, so every
    // parent is seen before its children.
    let mut depth = vec![0usize; slots];
    let mut deepest = 0usize;
    let mut elements = 0u64;
    let mut census = Census {
        texts: 0,
        images: 0,
    };
    for n in doc.descendants().filter(|n| n.is_element()) {
        let d = n.parent().map_or(0, |p| depth[p.id().get_usize()]) + 1;
        depth[n.id().get_usize()] = d;
        deepest = deepest.max(d);
        elements += 1;
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
            "text" => census.texts += 1,
            "image" => census.images += 1,
            _ => {}
        }
    }

    // CSS, as usvg reads it: the first text of every <style> element.
    let css: Vec<&str> = doc
        .descendants()
        .filter(|n| n.has_tag_name("style"))
        .filter_map(|n| n.text())
        .collect();
    let declarations = css
        .iter()
        .fold(0u64, |s, t| s.saturating_add(css_declaration_bound(t)));
    if declarations > MAX_SVG_CSS_DECLARATIONS {
        return Err(format!(
            "the SVG's <style> rules expand to about {declarations} CSS declarations (limit {MAX_SVG_CSS_DECLARATIONS}); export the SVG with presentation attributes instead of CSS"
        ));
    }
    let mut sheet = simplecss::StyleSheet::new();
    for t in &css {
        sheet.parse_more(t);
    }
    // Per rule: the matching bound for one element, and whether it can refer
    // to anything.
    let base = u64::try_from(deepest.max(1)).unwrap_or(u64::MAX);
    let mut rule_cost = Vec::with_capacity(sheet.rules.len());
    for rule in &sheet.rules {
        let (parts, descendant) = selector_shape(&rule.selector);
        if parts > MAX_CSS_SELECTOR_PARTS {
            return Err(format!(
                "a <style> selector has {parts} parts (limit {MAX_CSS_SELECTOR_PARTS}); simplify the CSS or export with presentation attributes"
            ));
        }
        let decls = u64::try_from(rule.declarations.len()).unwrap_or(u64::MAX);
        rule_cost.push(
            u64::try_from(parts)
                .unwrap_or(u64::MAX)
                .saturating_mul(base.saturating_pow(descendant))
                .saturating_add(decls),
        );
    }
    let css_refused = |work: u64| {
        format!(
            "matching the SVG's <style> rules would take about {work} selector steps (limit {MAX_SVG_CSS_WORK}); use fewer or simpler selectors (no long descendant chains), or export with presentation attributes"
        )
    };
    let refers =
        |rule: &simplecss::Rule| rule.declarations.iter().any(|d| d.value.contains("url("));
    let url_rules: Vec<usize> = (0..sheet.rules.len())
        .filter(|&i| refers(&sheet.rules[i]))
        .collect();
    let url_work = url_rules
        .iter()
        .fold(0u64, |s, &i| s.saturating_add(rule_cost[i]))
        .saturating_mul(elements);
    if url_work > MAX_SVG_CSS_WORK {
        return Err(css_refused(url_work));
    }

    // What each element makes usvg follow. Every element with a given id is a
    // possible target (usvg resolves `<use>` to the first and url() to the last).
    let mut ids: HashMap<&str, Vec<roxmltree::NodeId>> = HashMap::new();
    for n in doc.descendants().filter(|n| n.is_element()) {
        if let Some(id) = n.attribute("id") {
            ids.entry(id).or_default().push(n.id());
        }
    }
    let mut links: Vec<Links> = vec![Links::default(); slots];
    for n in doc.descendants().filter(|n| n.is_element()) {
        let l = &mut links[n.id().get_usize()];
        let tag = n.tag_name().name();
        for a in n.attributes() {
            match a.name() {
                "href" => {
                    if HREF_TAGS.contains(&tag) {
                        href_ids(a.value(), |id| {
                            for &t in ids.get(id).into_iter().flatten() {
                                if tag == "use" {
                                    l.copies.push(t);
                                } else {
                                    l.direct.push(t);
                                }
                            }
                        });
                    }
                }
                "style" => {
                    for d in simplecss::DeclarationTokenizer::from(a.value()) {
                        add_url(&ids, l, d.name, d.value);
                    }
                }
                name => add_url(&ids, l, name, a.value()),
            }
        }
        for &r in &url_rules {
            let rule = &sheet.rules[r];
            if rule.selector.matches(&CssNode(n)) {
                for d in &rule.declarations {
                    add_url(&ids, l, d.name, d.value);
                }
            }
        }
        for v in [&mut l.copies, &mut l.direct, &mut l.inherited] {
            v.sort_unstable_by_key(|id| id.get_usize());
            v.dedup();
        }
    }

    // One depth-first search over nesting and references with an explicit
    // stack (chains can be long; no recursion), computing per element, from
    // its dependencies, the expanded size (usvg's tree), the longest chain in
    // elements, and the work bound (see the constants).
    let mut size = vec![0u64; slots];
    let mut levels = vec![0usize; slots];
    let mut work = vec![0u64; slots];
    // 0 = not seen, 1 = on the current path, 2 = done.
    let mut state = vec![0u8; slots];
    let root = doc.root_element();
    let mut stack = vec![(root.id(), false)];
    while let Some((id, expanded)) = stack.pop() {
        let Some(node) = doc.get_node(id) else {
            continue;
        };
        let i = id.get_usize();
        let l = &links[i];
        if expanded {
            let attrs = u64::try_from(node.attributes().len()).unwrap_or(u64::MAX);
            let (mut s, mut w, mut below) = (1u64, attrs.saturating_add(1), 0usize);
            for d in structural(node, &links) {
                let d = d.get_usize();
                s = s.saturating_add(size[d]);
                w = w.saturating_add(work[d]);
                below = below.max(levels[d]);
            }
            let mut chain = below;
            for r in &l.direct {
                w = w.saturating_add(work[r.get_usize()]);
                chain = chain.max(levels[r.get_usize()]);
            }
            for r in &l.inherited {
                w = w.saturating_add(s.saturating_mul(work[r.get_usize()]));
                chain = chain.max(below + levels[r.get_usize()]);
            }
            let chain = chain + 1;
            if s > MAX_SVG_ELEMENTS {
                return Err(format!(
                    "the SVG expands to more than {MAX_SVG_ELEMENTS} elements through <use> references; simplify it or expand the references in your editor"
                ));
            }
            if chain > MAX_SVG_DEPTH {
                return Err(format!(
                    "groups and references (<use>, clip paths, masks, patterns, gradients, filters) chain more than {MAX_SVG_DEPTH} levels deep; simplify the SVG in your editor (Inkscape: Edit > Clone > Unlink Clone, Object > Ungroup, Path > Intersection for clips)"
                ));
            }
            if w > MAX_SVG_WORK {
                return Err(format!(
                    "the SVG's references to patterns, clip paths, masks or filters could make usvg build about {w} elements and attributes (limit {MAX_SVG_WORK}); simplify them in your editor"
                ));
            }
            size[i] = s;
            levels[i] = chain;
            work[i] = w;
            state[i] = 2;
            continue;
        }
        match state[i] {
            2 => continue,
            1 => {
                return Err(
                    "an href or url(#...) reference refers back to itself or to an element that contains it"
                        .into(),
                )
            }
            _ => {}
        }
        state[i] = 1;
        stack.push((id, true));
        for d in structural(node, &links)
            .chain(l.direct.iter().copied())
            .chain(l.inherited.iter().copied())
        {
            if state[d.get_usize()] != 2 {
                stack.push((d, false));
            }
        }
    }
    // usvg matches every rule against every element of its tree.
    let css_work = rule_cost
        .iter()
        .fold(0u64, |s, c| s.saturating_add(*c))
        .saturating_mul(size[root.id().get_usize()]);
    if css_work > MAX_SVG_CSS_WORK {
        return Err(css_refused(css_work));
    }
    Ok(census)
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

/// A paint, as a key for counting different fills.
fn paint_key(paint: &usvg::Paint) -> String {
    match paint {
        usvg::Paint::Color(c) => format!("#{:02x}{:02x}{:02x}", c.red, c.green, c.blue),
        usvg::Paint::LinearGradient(g) => format!("linear {}", g.id()),
        usvg::Paint::RadialGradient(g) => format!("radial {}", g.id()),
        usvg::Paint::Pattern(p) => format!("pattern {}", p.id()),
    }
}

/// Parse SVG bytes into filled shapes. Errors are messages for the user; no
/// input makes this panic or exhaust the stack (tests/eval_relief.rs runs the
/// hostile fixtures, the reference chains on a 1 MiB thread).
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
    check_attribute_counts(text)?;
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
    let census = prescan(&xml)?;
    let tree = usvg::Tree::from_xmltree(&xml, &options()).map_err(|e| match e {
        usvg::Error::InvalidSize => {
            "the SVG has no valid size: give the <svg> element a viewBox (or width and height) larger than zero"
                .to_string()
        }
        e => format!("usvg could not read the SVG: {e}"),
    })?;

    let mut parsed = Parsed {
        texts: census.texts,
        images: census.images,
        ..Parsed::default()
    };
    let mut paints = BTreeSet::new();
    // The order of shapes does not matter: the caller unions them. Each group
    // comes with the product of its ancestors' opacities and its own.
    let mut stack: Vec<(&usvg::Group, f32)> = vec![(tree.root(), tree.root().opacity().get())];
    while let Some((group, opacity)) = stack.pop() {
        parsed.clip_paths += usize::from(group.clip_path().is_some());
        parsed.masks += usize::from(group.mask().is_some());
        parsed.filters += usize::from(!group.filters().is_empty());
        for node in group.children() {
            match node {
                usvg::Node::Group(g) => stack.push((g, opacity * g.opacity().get())),
                usvg::Node::Path(p) => {
                    parsed.strokes += usize::from(p.stroke().is_some());
                    let Some(fill) = p.fill() else { continue };
                    // Invisible paths (hidden, or transparent through their fill
                    // or any group) are not artwork and do not change the fit.
                    if !p.is_visible() || opacity <= 0.0 || fill.opacity().get() <= 0.0 {
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
                        paints.insert(paint_key(fill.paint()));
                        parsed.shapes.push(Shape { rule, subpaths });
                    }
                }
                // Images are never resolved and text is not converted without
                // fonts; both were counted from the XML.
                usvg::Node::Image(_) | usvg::Node::Text(_) => {}
            }
        }
    }
    parsed.fill_paints = paints.len();
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
        assert_eq!(p.fill_paints, 1);
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
        // A cycle of three clip paths, which usvg's own repair (one step deep)
        // misses.
        let cycle = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1">
            <clipPath id="a" clip-path="url(#b)"><rect width="1" height="1"/></clipPath>
            <clipPath id="b" clip-path="url(#c)"><rect width="1" height="1"/></clipPath>
            <clipPath id="c" clip-path="url(#a)"><rect width="1" height="1"/></clipPath>
            <rect width="1" height="1" clip-path="url(#a)"/></svg>"##;
        let e = parse(cycle).expect_err("a clip cycle is refused");
        assert!(e.contains("refers back"), "{e}");
    }

    /// An SVG whose `kind` of link is chained `n` times (the reviewer's probes
    /// of backlog item 9, and the kinds usvg follows by recursion).
    pub(crate) fn chain(kind: &str, n: usize) -> String {
        let mut s = String::from(
            r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10"><defs>"#,
        );
        let mut body = String::new();
        let attr = match kind {
            "clip" | "clipchild" => "clip-path",
            "mask" | "maskinner" => "mask",
            "pattern" => "fill",
            _ => "filter",
        };
        match kind {
            "nest" | "svgnest" => {
                let (open, close) = if kind == "nest" {
                    ("<g>", "</g>")
                } else {
                    (r#"<svg width="10" height="10">"#, "</svg>")
                };
                body.push_str(&open.repeat(n));
                body.push_str(r#"<rect width="10" height="10"/>"#);
                body.push_str(&close.repeat(n));
            }
            "use" | "usebare" => {
                for i in 0..n {
                    let j = i + 1;
                    s.push_str(&if kind == "use" {
                        format!(r##"<g id="g{i}"><use xlink:href="#g{j}"/></g>"##)
                    } else {
                        format!(r##"<use id="g{i}" xlink:href="#g{j}"/>"##)
                    });
                }
                s.push_str(&format!(r#"<rect id="g{n}" width="10" height="10"/>"#));
                body.push_str(r##"<use xlink:href="#g0"/>"##);
            }
            _ => {
                for i in 0..n {
                    let j = i + 1;
                    s.push_str(&match kind {
                        "clip" => format!(r##"<clipPath id="c{i}" clip-path="url(#c{j})"><rect width="10" height="10"/></clipPath>"##),
                        "clipchild" => format!(r##"<clipPath id="c{i}"><rect width="10" height="10" clip-path="url(#c{j})"/></clipPath>"##),
                        "mask" => format!(r##"<mask id="c{i}" mask="url(#c{j})"><rect width="10" height="10" fill="white"/></mask>"##),
                        "maskinner" => format!(r##"<mask id="c{i}"><rect width="10" height="10" fill="white" mask="url(#c{j})"/></mask>"##),
                        "pattern" => format!(r##"<pattern id="c{i}" width="1" height="1" patternUnits="userSpaceOnUse"><rect width="1" height="1" fill="url(#c{j})"/></pattern>"##),
                        _ => format!(r##"<filter id="c{i}"><feImage href="#e{i}"/></filter><rect id="e{i}" width="1" height="1" filter="url(#c{j})"/>"##),
                    });
                }
                s.push_str(&match kind {
                    "clip" | "clipchild" => format!(r#"<clipPath id="c{n}"><rect width="10" height="10"/></clipPath>"#),
                    "mask" | "maskinner" => format!(r#"<mask id="c{n}"><rect width="10" height="10" fill="white"/></mask>"#),
                    "pattern" => format!(r#"<pattern id="c{n}" width="1" height="1" patternUnits="userSpaceOnUse"><rect width="1" height="1"/></pattern>"#),
                    _ => format!(r#"<filter id="c{n}"><feGaussianBlur stdDeviation="1"/></filter>"#),
                });
                body.push_str(&format!(
                    r##"<rect width="10" height="10" {attr}="url(#c0)"/>"##
                ));
            }
        }
        s.push_str("</defs>");
        s.push_str(&body);
        s.push_str("</svg>");
        s
    }

    /// Every kind of chain usvg follows by recursion is accepted up to the limit
    /// and refused past it, and every accepted one fits a 1 MiB stack (the
    /// wasm32 default) in this debug build, whose frames are larger than a
    /// release build's. Without the limit, chains of a few hundred links
    /// overflowed such a stack and aborted the process.
    #[test]
    fn reference_chains_stay_within_a_wasm_stack() {
        const KINDS: [&str; 10] = [
            "nest",
            "svgnest",
            "use",
            "usebare",
            "clip",
            "clipchild",
            "mask",
            "maskinner",
            "pattern",
            "filter",
        ];
        let worker = std::thread::Builder::new()
            .stack_size(1 << 20)
            .spawn(|| {
                let mut longest = Vec::new();
                for kind in KINDS {
                    let mut accepted = 0;
                    for n in 1..=2 * MAX_SVG_DEPTH {
                        match parse(chain(kind, n).as_bytes()) {
                            Ok(p) => {
                                assert_eq!(p.shapes.len(), 1, "{kind} {n}");
                                accepted = n;
                            }
                            Err(e) => {
                                assert!(e.contains("levels deep"), "{kind} {n}: {e}");
                                break;
                            }
                        }
                    }
                    longest.push((kind, accepted));
                    // The reviewer's probes, far past the limit, are refused.
                    let e = parse(chain(kind, 1000).as_bytes()).expect_err("refused");
                    assert!(e.contains("levels deep"), "{kind}: {e}");
                }
                longest
            })
            .expect("thread");
        let longest = worker.join().expect("no stack overflow on 1 MiB");
        eprintln!("longest accepted chains: {longest:?}");
        for (kind, n) in longest {
            // Every kind allows a reasonable chain (at least 20 links).
            assert!(n >= 20, "{kind}: only {n} links accepted");
        }
    }

    #[test]
    fn url_references_are_read_generously() {
        let mut got = Vec::new();
        url_ids("url(#a) url( '#b c' ) url(\"#d\")x url(e) url(#f", |id| {
            got.push(id)
        });
        assert_eq!(got, ["a", "a", "b c", "b", "d", "d", "f", "f"]);
    }

    #[test]
    fn css_references_and_bounds() {
        // A clip path set through a class, as Illustrator exports it, counts as a
        // reference: a cycle through CSS is found.
        let svg = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1">
            <style>.c { clip-path: url(#k) }</style>
            <clipPath id="k"><rect class="c" width="1" height="1"/></clipPath>
            <rect class="c" width="1" height="1"/></svg>"##;
        let e = parse(svg).expect_err("a cycle through CSS");
        assert!(e.contains("refers back"), "{e}");
        // Without the cycle the same file builds.
        let ok = br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1">
            <style>.c { clip-path: url(#k) } .cls-1 { fill: #231f20 }</style>
            <clipPath id="k"><rect width="1" height="1"/></clipPath>
            <rect class="c cls-1" width="1" height="1"/></svg>"##;
        let p = parse(ok).expect("builds");
        assert_eq!(p.shapes.len(), 1);
        // A selector list that multiplies declarations.
        let mut list =
            String::from(r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 1 1"><style>"#);
        list.push_str(&"a,".repeat(2000));
        list.push_str("a{");
        list.push_str(&"fill:red;".repeat(100));
        list.push_str(r#"}</style><rect width="1" height="1"/></svg>"#);
        let e = parse(list.as_bytes()).expect_err("too many declarations");
        assert!(e.contains("CSS declarations"), "{e}");
        assert_eq!(css_declaration_bound("a,b{x:1;y:2} c{z:3}"), 5);
        assert_eq!(css_declaration_bound("a{x:1;y:2"), 2);
    }

    #[test]
    fn attribute_counts_skip_comments_and_quotes() {
        assert!(check_attribute_counts(r#"<a b="=" c='>='><!-- = = --></a>"#).is_ok());
        let mut s = String::from("<svg ");
        for i in 0..=MAX_SVG_ATTRIBUTES {
            s.push_str(&format!("a{i}=\"x>y\" "));
        }
        s.push_str("/>");
        let e = check_attribute_counts(&s).expect_err("too many");
        assert!(e.contains("attributes"), "{e}");
    }
}
