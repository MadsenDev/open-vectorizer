//! Behavioural fixtures for logo-shaped input.
//!
//! These assert on the geometry the engine produces, via the [`Document`] API,
//! rather than on the text of the SVG. Matching serialized path data made the
//! old versions of these tests fail the moment the writer learned to emit
//! `<rect>` for a square or to drop redundant command letters — neither of
//! which is a behaviour change worth a test failure.

use image::{Rgba, RgbaImage};
use png2svg_core::path::{Outline, Segment};
use png2svg_core::svg::Document;
use png2svg_core::{vectorize_image, VectorizeMode, VectorizeOptions};

fn options(mode: VectorizeMode) -> VectorizeOptions {
    VectorizeOptions {
        mode,
        ..VectorizeOptions::default()
    }
}

/// Every distinct fill in the document.
fn fills(document: &Document) -> Vec<[u8; 4]> {
    let mut colors: Vec<[u8; 4]> = document.shapes.iter().map(|shape| shape.color).collect();
    colors.sort_unstable();
    colors.dedup();
    colors
}

#[test]
fn preserves_transparent_logo_cutouts() {
    // A 5x5 mark with a single-pixel hole punched out of the middle.
    let image = RgbaImage::from_fn(7, 7, |x, y| {
        let in_mark = (1..=5).contains(&x) && (1..=5).contains(&y);
        let in_hole = x == 3 && y == 3;
        if in_mark && !in_hole {
            Rgba([20, 20, 20, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let document = vectorize_image(
        &image,
        &VectorizeOptions {
            colors: 2,
            mode: VectorizeMode::PixelArt,
            ..VectorizeOptions::default()
        },
    );

    assert_eq!(document.shapes.len(), 1, "the mark is a single shape");
    assert_eq!(
        document.shapes[0].holes.len(),
        1,
        "the cutout should survive as a hole"
    );

    // Holes need even-odd fill to actually read as holes.
    let svg = document.to_svg();
    assert!(svg.contains("fill-rule=\"evenodd\""), "got {svg}");

    // Area is the mark minus the hole.
    let area = document.shapes[0].area();
    assert!((area - 24.0).abs() < 0.01, "area was {area}, expected 24");
}

#[test]
fn keeps_two_colour_logo_regions_separate() {
    let image = RgbaImage::from_fn(8, 4, |x, _| {
        if x < 4 {
            Rgba([220, 40, 40, 255])
        } else {
            Rgba([40, 90, 220, 255])
        }
    });

    let document = vectorize_image(
        &image,
        &VectorizeOptions {
            colors: 2,
            detail: 1.0,
            mode: VectorizeMode::PixelArt,
            ..VectorizeOptions::default()
        },
    );

    assert_eq!(document.shapes.len(), 2, "one shape per colour");
    assert_eq!(fills(&document).len(), 2, "two distinct fills");
    assert_eq!(document.to_svg().matches("<g fill=").count(), 2);
}

#[test]
fn pixel_mode_keeps_edges_on_the_pixel_grid() {
    let image = RgbaImage::from_fn(4, 4, |x, y| {
        if x == y {
            Rgba([0, 0, 0, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let document = vectorize_image(
        &image,
        &VectorizeOptions {
            colors: 2,
            mode: VectorizeMode::PixelArt,
            ..VectorizeOptions::default()
        },
    );

    assert!(!document.shapes.is_empty());
    for shape in &document.shapes {
        for outline in std::iter::once(&shape.outer).chain(shape.holes.iter()) {
            let contour = outline.to_contour();
            assert!(
                contour
                    .segments
                    .iter()
                    .all(|segment| matches!(segment, Segment::Line { .. })),
                "pixel mode must not smooth edges into curves"
            );
            for point in contour.flatten(0.01) {
                assert!(
                    (point.x.round() - point.x).abs() < 1e-9
                        && (point.y.round() - point.y).abs() < 1e-9,
                    "pixel art vertex left the grid at {point:?}"
                );
            }
        }
    }
}

#[test]
fn logo_mode_smooths_a_round_mark_into_exact_circles() {
    // An anti-aliased ring. The README singles this out as a case the previous
    // engine could not handle; it should now come back as two exact circles.
    let image = RgbaImage::from_fn(64, 64, |x, y| {
        let dx = x as f64 + 0.5 - 32.0;
        let dy = y as f64 + 0.5 - 32.0;
        let distance = (dx * dx + dy * dy).sqrt();
        // Smooth ramp across each boundary, standing in for anti-aliasing.
        let coverage = ((distance - 12.0).clamp(0.0, 1.0)) * ((24.0 - distance).clamp(0.0, 1.0));
        if coverage <= 0.0 {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([30, 30, 34, (coverage * 255.0).round() as u8])
        }
    });

    let document = vectorize_image(&image, &options(VectorizeMode::Logo));

    assert_eq!(document.shapes.len(), 1, "a ring is one shape");
    assert_eq!(document.shapes[0].holes.len(), 1, "with one hole");
    assert_eq!(
        document.stats().circles,
        2,
        "outer and inner should both be circles, got {:?}",
        document.stats()
    );
}

#[test]
fn logo_mode_removes_isolated_specks_by_default() {
    let image = RgbaImage::from_fn(8, 8, |x, y| {
        let in_mark = (1..=4).contains(&x) && (1..=4).contains(&y);
        let is_speck = (x, y) == (7, 0) || (x, y) == (0, 7);
        if in_mark || is_speck {
            Rgba([10, 10, 10, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let document = vectorize_image(&image, &options(VectorizeMode::Logo));

    assert_eq!(
        document.shapes.len(),
        1,
        "single-pixel specks should be dropped at default detail"
    );
    // The 4x4 mark is an axis-aligned square, so it should come back as a rect.
    assert!(
        matches!(document.shapes[0].outer, Outline::Rect { .. }),
        "expected a rect, got {:?}",
        document.shapes[0].outer
    );
}

#[test]
fn high_detail_logo_mode_preserves_tiny_components() {
    let image = RgbaImage::from_fn(8, 8, |x, y| {
        let in_mark = (1..=4).contains(&x) && (1..=4).contains(&y);
        let is_detail = (x, y) == (7, 0);
        if in_mark || is_detail {
            Rgba([10, 10, 10, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let document = vectorize_image(
        &image,
        &VectorizeOptions {
            detail: 0.9,
            mode: VectorizeMode::Logo,
            ..VectorizeOptions::default()
        },
    );

    assert_eq!(
        document.shapes.len(),
        2,
        "high detail should keep the single-pixel mark"
    );
}

#[test]
fn logo_mode_merges_near_duplicate_brand_colours() {
    let image = RgbaImage::from_fn(8, 4, |x, _| {
        if x < 4 {
            Rgba([220, 40, 40, 255])
        } else {
            Rgba([225, 43, 39, 255])
        }
    });

    let document = vectorize_image(
        &image,
        &VectorizeOptions {
            colors: 4,
            detail: 0.6,
            mode: VectorizeMode::Logo,
            ..VectorizeOptions::default()
        },
    );

    assert_eq!(
        fills(&document).len(),
        1,
        "two shades of the same red should collapse to one fill"
    );
}

#[test]
fn pixel_mode_keeps_near_duplicate_colours_distinct() {
    let image = RgbaImage::from_fn(8, 4, |x, _| {
        if x < 4 {
            Rgba([220, 40, 40, 255])
        } else {
            Rgba([225, 43, 39, 255])
        }
    });

    let document = vectorize_image(
        &image,
        &VectorizeOptions {
            colors: 4,
            detail: 1.0,
            mode: VectorizeMode::PixelArt,
            ..VectorizeOptions::default()
        },
    );

    assert_eq!(
        fills(&document).len(),
        2,
        "pixel art must preserve exact colours"
    );
}

#[test]
fn vectorization_output_is_deterministic() {
    let image = RgbaImage::from_fn(10, 10, |x, y| {
        if (1..=4).contains(&x) && (1..=4).contains(&y) {
            Rgba([220, 40, 40, 255])
        } else if (5..=8).contains(&x) && (5..=8).contains(&y) {
            Rgba([40, 90, 220, 255])
        } else if (x, y) == (8, 1) {
            Rgba([20, 20, 20, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let options = VectorizeOptions {
        colors: 4,
        detail: 0.7,
        mode: VectorizeMode::Logo,
        ..VectorizeOptions::default()
    };

    let first = vectorize_image(&image, &options).to_svg();
    for _ in 0..10 {
        assert_eq!(first, vectorize_image(&image, &options).to_svg());
    }
}

#[test]
fn an_anti_aliased_edge_lands_between_pixels() {
    // A vertical edge at x = 8.5. An integer-grid tracer must put it at 8 or 9;
    // reading coverage should place it within a hundredth of a pixel.
    let image = RgbaImage::from_fn(20, 8, |x, _| {
        let coverage = (8.5 - x as f64).clamp(0.0, 1.0);
        if coverage <= 0.0 {
            Rgba([0, 0, 0, 0])
        } else {
            Rgba([15, 15, 15, (coverage * 255.0).round() as u8])
        }
    });

    let document = vectorize_image(&image, &options(VectorizeMode::Logo));
    assert_eq!(document.shapes.len(), 1);

    let bounds = document.shapes[0].outer.bounds();
    assert!(
        (bounds.max_x - 8.5).abs() < 0.02,
        "edge landed at {}, expected 8.5",
        bounds.max_x
    );
}
