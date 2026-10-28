//! Plain 2D regions: fill-rule resolution and booleans on sets of polygons, for
//! artwork (docs/plan/06 section 5) and other planar work that no 3D solid needs.
//!
//! The work is done by Clipper2 (`clipper2-rust`, BSL-1.0, a pure-Rust port), the
//! library both Manifold backends use for their own cross-sections, so the result
//! does not depend on the kernel backend and builds for wasm32. Every function
//! returns a [`CrossSection`] in the form the kernel expects: non-overlapping
//! outer contours counter-clockwise and holes clockwise, so the Positive and the
//! NonZero fill rules both read it as the same region and [`CrossSection::area`]
//! is its area. Coordinates snap to a grid of 1e-8 (Clipper2's decimal
//! precision 8, the one manifold-rust's `CrossSection` uses).

use clipper2_rust::{
    difference_d, intersect_d, union_subjects_d, FillRule as ClipperFill, PathD, PathsD, Point,
};

use crate::{CrossSection, Polygon};

/// Decimal digits Clipper2 keeps (the precision of manifold-rust's cross-sections).
const PRECISION: i32 = 8;

/// How overlapping or self-intersecting contours fill (SVG `fill-rule`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FillRule {
    /// Inside where the winding number is not zero (the SVG default).
    NonZero,
    /// Inside where a ray crosses an odd number of edges.
    EvenOdd,
}

fn to_paths(polygons: &[Polygon]) -> PathsD {
    polygons
        .iter()
        .map(|p| -> PathD { p.points.iter().map(|&[x, y]| Point::new(x, y)).collect() })
        .collect()
}

fn from_paths(paths: PathsD) -> CrossSection {
    CrossSection {
        polygons: paths
            .into_iter()
            .map(|p| Polygon {
                points: p.into_iter().map(|q| [q.x, q.y]).collect(),
            })
            .collect(),
    }
}

/// The region `contours` enclose under `rule`, with overlaps and
/// self-intersections resolved. Contours may run either way round; under
/// [`FillRule::EvenOdd`] two nested contours drawn the same way make a ring.
pub fn fill(contours: &[Polygon], rule: FillRule) -> CrossSection {
    let rule = match rule {
        FillRule::NonZero => ClipperFill::NonZero,
        FillRule::EvenOdd => ClipperFill::EvenOdd,
    };
    from_paths(union_subjects_d(&to_paths(contours), rule, PRECISION))
}

/// The union of regions already in kernel form (outer counter-clockwise, holes
/// clockwise).
pub fn union_all(sections: &[CrossSection]) -> CrossSection {
    let polygons: Vec<Polygon> = sections
        .iter()
        .flat_map(|s| s.polygons.iter().cloned())
        .collect();
    from_paths(union_subjects_d(
        &to_paths(&polygons),
        ClipperFill::Positive,
        PRECISION,
    ))
}

/// `a` minus `b`, both in kernel form.
pub fn difference(a: &CrossSection, b: &CrossSection) -> CrossSection {
    from_paths(difference_d(
        &to_paths(&a.polygons),
        &to_paths(&b.polygons),
        ClipperFill::Positive,
        PRECISION,
    ))
}

/// The part of `a` inside `b`, both in kernel form.
pub fn intersection(a: &CrossSection, b: &CrossSection) -> CrossSection {
    from_paths(intersect_d(
        &to_paths(&a.polygons),
        &to_paths(&b.polygons),
        ClipperFill::Positive,
        PRECISION,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Unit-free test shapes (no brick dimension is involved).
    fn square(min: [f64; 2], side: f64) -> Polygon {
        Polygon::rect(min, [side, side])
    }

    #[test]
    fn nested_same_way_contours_are_a_ring_under_even_odd_only() {
        let outer = Polygon::circle([0.0, 0.0], 10.0, 64);
        let inner = Polygon::circle([0.0, 0.0], 6.0, 64);
        let both = [outer.clone(), inner.clone()];
        let ring = fill(&both, FillRule::EvenOdd);
        let disc = fill(&both, FillRule::NonZero);
        let want_ring = outer.signed_area() - inner.signed_area();
        assert!((ring.area() - want_ring).abs() < 1e-6, "{}", ring.area());
        assert!((disc.area() - outer.signed_area()).abs() < 1e-6);
        // Kernel form: one counter-clockwise outer and one clockwise hole.
        let mut signs: Vec<bool> = ring
            .polygons
            .iter()
            .map(|p| p.signed_area() > 0.0)
            .collect();
        signs.sort();
        assert_eq!(signs, vec![false, true]);
    }

    #[test]
    fn clockwise_contours_fill_under_both_rules() {
        // SVG files draw shapes either way round; the result is always positive.
        let cw = square([0.0, 0.0], 2.0).reversed();
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            let r = fill(std::slice::from_ref(&cw), rule);
            assert!((r.area() - 4.0).abs() < 1e-9, "{rule:?}: {}", r.area());
        }
    }

    #[test]
    fn a_self_intersecting_star_differs_between_the_rules() {
        // A pentagram drawn as one path: its centre pentagon has winding number 2.
        let points = (0..5)
            .map(|i| {
                let a =
                    std::f64::consts::FRAC_PI_2 + f64::from(i * 2) * std::f64::consts::TAU / 5.0;
                [a.cos(), a.sin()]
            })
            .collect();
        let star = [Polygon { points }];
        let nonzero = fill(&star, FillRule::NonZero).area();
        let evenodd = fill(&star, FillRule::EvenOdd).area();
        assert!(nonzero > evenodd + 0.1, "{nonzero} vs {evenodd}");
    }

    #[test]
    fn booleans_of_overlapping_squares() {
        let a = fill(&[square([0.0, 0.0], 2.0)], FillRule::NonZero);
        let b = fill(&[square([1.0, 1.0], 2.0)], FillRule::NonZero);
        assert!((union_all(&[a.clone(), b.clone()]).area() - 7.0).abs() < 1e-9);
        assert!((intersection(&a, &b).area() - 1.0).abs() < 1e-9);
        assert!((difference(&a, &b).area() - 3.0).abs() < 1e-9);
        assert!(difference(&a, &a).polygons.is_empty());
    }
}
