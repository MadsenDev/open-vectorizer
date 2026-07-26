//! Ground-truth quality tests.
//!
//! Every case here starts from geometry we *know*, renders it with proper
//! anti-aliasing the way any real rasterizer would, and then checks that
//! vectorizing the pixels recovers the original geometry. Because the source
//! geometry is known exactly, these are measurements rather than opinions:
//! "did the circle come back as a circle, with the right radius, to within a
//! tenth of a pixel".
//!
//! This is also the harness for comparing against any other engine: render the
//! same inputs, run them through, and compare recovered geometry, node counts
//! and [`png2svg_core::accuracy`].

use image::{Rgba, RgbaImage};
use png2svg_core::geom::Point;
use png2svg_core::path::{Outline, Segment};
use png2svg_core::svg::Document;
use png2svg_core::{accuracy, vectorize_image, VectorizeMode, VectorizeOptions};

/// Sub-samples per axis when rendering ground truth. 16x16 per pixel puts the
/// rendered coverage well inside a thousandth of a pixel of exact.
const SUPERSAMPLE: usize = 16;

type InsideTest = Box<dyn Fn(f64, f64) -> bool>;

struct Layer {
    color: [u8; 4],
    inside: InsideTest,
}

/// Render layers with analytic anti-aliasing. Later layers paint over earlier
/// ones, and uncovered area stays transparent.
fn render(width: u32, height: u32, layers: &[Layer]) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);
    let step = 1.0 / SUPERSAMPLE as f64;

    for y in 0..height {
        for x in 0..width {
            // Accumulate in premultiplied space, which is where compositing is
            // linear, then convert back.
            let mut accumulated = [0.0f64; 4];
            for sy in 0..SUPERSAMPLE {
                for sx in 0..SUPERSAMPLE {
                    let px = x as f64 + (sx as f64 + 0.5) * step;
                    let py = y as f64 + (sy as f64 + 0.5) * step;

                    let mut hit: Option<[u8; 4]> = None;
                    for layer in layers {
                        if (layer.inside)(px, py) {
                            hit = Some(layer.color);
                        }
                    }
                    if let Some(color) = hit {
                        let alpha = color[3] as f64 / 255.0;
                        accumulated[0] += color[0] as f64 / 255.0 * alpha;
                        accumulated[1] += color[1] as f64 / 255.0 * alpha;
                        accumulated[2] += color[2] as f64 / 255.0 * alpha;
                        accumulated[3] += alpha;
                    }
                }
            }

            let samples = (SUPERSAMPLE * SUPERSAMPLE) as f64;
            let alpha = accumulated[3] / samples;
            let pixel = if alpha <= 0.0 {
                Rgba([0, 0, 0, 0])
            } else {
                Rgba([
                    ((accumulated[0] / samples / alpha) * 255.0).round().clamp(0.0, 255.0) as u8,
                    ((accumulated[1] / samples / alpha) * 255.0).round().clamp(0.0, 255.0) as u8,
                    ((accumulated[2] / samples / alpha) * 255.0).round().clamp(0.0, 255.0) as u8,
                    (alpha * 255.0).round().clamp(0.0, 255.0) as u8,
                ])
            };
            image.put_pixel(x, y, pixel);
        }
    }

    image
}

fn disc(cx: f64, cy: f64, r: f64) -> InsideTest {
    Box::new(move |x, y| (x - cx).powi(2) + (y - cy).powi(2) <= r * r)
}

fn ring(cx: f64, cy: f64, outer: f64, inner: f64) -> InsideTest {
    Box::new(move |x, y| {
        let d = (x - cx).powi(2) + (y - cy).powi(2);
        d <= outer * outer && d > inner * inner
    })
}

fn rect(x0: f64, y0: f64, w: f64, h: f64) -> InsideTest {
    Box::new(move |x, y| x >= x0 && x < x0 + w && y >= y0 && y < y0 + h)
}

fn polygon(vertices: Vec<Point>) -> InsideTest {
    Box::new(move |x, y| png2svg_core::geom::point_in_polygon(Point::new(x, y), &vertices))
}

fn rounded_rect(x0: f64, y0: f64, w: f64, h: f64, radius: f64) -> InsideTest {
    Box::new(move |x, y| {
        if x < x0 || y < y0 || x >= x0 + w || y >= y0 + h {
            return false;
        }
        // Clamp into the inner rectangle; outside the corner arcs this is the
        // point itself, so the test reduces to the plain rectangle.
        let cx = x.clamp(x0 + radius, x0 + w - radius);
        let cy = y.clamp(y0 + radius, y0 + h - radius);
        (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
    })
}

fn star(cx: f64, cy: f64, outer: f64, inner: f64, points: usize) -> InsideTest {
    let mut vertices = Vec::new();
    for index in 0..points * 2 {
        let radius = if index % 2 == 0 { outer } else { inner };
        // Start at the top so the shape looks like a conventional star.
        let angle = index as f64 / (points * 2) as f64 * std::f64::consts::TAU
            - std::f64::consts::FRAC_PI_2;
        vertices.push(Point::new(
            cx + radius * angle.cos(),
            cy + radius * angle.sin(),
        ));
    }
    polygon(vertices)
}

fn logo_options() -> VectorizeOptions {
    VectorizeOptions {
        mode: VectorizeMode::Logo,
        colors: 8,
        detail: 0.6,
        smoothness: 0.5,
        tolerance: 1.5,
    }
}

fn find_circle(document: &Document) -> Option<(Point, f64)> {
    document.shapes.iter().find_map(|shape| match shape.outer {
        Outline::Circle { center, radius } => Some((center, radius)),
        _ => None,
    })
}

fn outer_segments(document: &Document) -> Vec<Segment> {
    document
        .shapes
        .first()
        .map(|shape| shape.outer.to_contour().segments)
        .unwrap_or_default()
}

// ---------------------------------------------------------------------------
// Primitive recovery
// ---------------------------------------------------------------------------

#[test]
fn a_rendered_circle_comes_back_as_a_circle_element() {
    let image = render(
        200,
        200,
        &[Layer {
            color: [220, 40, 40, 255],
            inside: disc(100.0, 100.0, 70.0),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    assert_eq!(document.shapes.len(), 1, "expected one shape");
    let (center, radius) = find_circle(&document).unwrap_or_else(|| {
        panic!(
            "expected a circle element, got {:?}",
            document.shapes[0].outer
        )
    });

    assert!(
        (center.x - 100.0).abs() < 0.1 && (center.y - 100.0).abs() < 0.1,
        "centre recovered as {center:?}"
    );
    assert!(
        (radius - 70.0).abs() < 0.1,
        "radius recovered as {radius}, expected 70"
    );
    assert!(svg_contains(&document, "<circle"));
}

#[test]
fn sub_pixel_position_and_radius_are_recovered() {
    // Nothing here lands on a pixel boundary. An integer-grid tracer cannot do
    // better than half a pixel; reading coverage should do far better.
    let image = render(
        160,
        160,
        &[Layer {
            color: [30, 60, 200, 255],
            inside: disc(80.37, 79.62, 51.45),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    let (center, radius) =
        find_circle(&document).expect("a circle should still be recognised off-grid");
    assert!(
        (center.x - 80.37).abs() < 0.1,
        "centre x recovered as {}",
        center.x
    );
    assert!(
        (center.y - 79.62).abs() < 0.1,
        "centre y recovered as {}",
        center.y
    );
    assert!(
        (radius - 51.45).abs() < 0.1,
        "radius recovered as {radius}, expected 51.45"
    );
}

#[test]
fn a_rendered_ring_comes_back_as_two_circles() {
    // The README calls out smooth rings as a failure case.
    let image = render(
        200,
        200,
        &[Layer {
            color: [20, 20, 24, 255],
            inside: ring(100.0, 100.0, 74.0, 46.0),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    assert_eq!(document.shapes.len(), 1, "a ring is one shape with one hole");
    let shape = &document.shapes[0];
    assert_eq!(shape.holes.len(), 1, "the ring should have a hole");

    let stats = document.stats();
    assert_eq!(
        stats.circles, 2,
        "both the outer and inner boundary should be exact circles, got {stats:?}"
    );

    match (&shape.outer, &shape.holes[0]) {
        (
            &Outline::Circle {
                center: outer_center,
                radius: outer_radius,
            },
            &Outline::Circle {
                center: inner_center,
                radius: inner_radius,
            },
        ) => {
            assert!((outer_radius - 74.0).abs() < 0.1, "outer {outer_radius}");
            assert!((inner_radius - 46.0).abs() < 0.1, "inner {inner_radius}");
            assert!(outer_center.distance(inner_center) < 0.1, "concentric");
        }
        other => panic!("expected two circles, got {other:?}"),
    }
}

#[test]
fn an_ellipse_comes_back_as_an_ellipse_element() {
    let image = render(
        220,
        160,
        &[Layer {
            color: [90, 180, 60, 255],
            inside: Box::new(|x: f64, y: f64| {
                ((x - 110.0) / 90.0).powi(2) + ((y - 80.0) / 50.0).powi(2) <= 1.0
            }),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    let stats = document.stats();
    assert_eq!(
        stats.ellipses, 1,
        "expected one ellipse, got {stats:?} with outer {:?}",
        document.shapes[0].outer
    );

    match document.shapes[0].outer {
        Outline::Ellipse { rx, ry, .. } => {
            let (major, minor) = if rx >= ry { (rx, ry) } else { (ry, rx) };
            assert!((major - 90.0).abs() < 0.3, "major axis {major}");
            assert!((minor - 50.0).abs() < 0.3, "minor axis {minor}");
        }
        ref other => panic!("expected an ellipse, got {other:?}"),
    }
}

#[test]
fn an_axis_aligned_square_comes_back_as_a_rect() {
    let image = render(
        120,
        120,
        &[Layer {
            color: [10, 10, 10, 255],
            inside: rect(24.5, 30.25, 60.0, 48.0),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    let stats = document.stats();
    assert_eq!(stats.rects, 1, "expected a rect, got {stats:?}");

    match document.shapes[0].outer {
        Outline::Rect {
            x,
            y,
            width,
            height,
        } => {
            assert!((x - 24.5).abs() < 0.1, "x {x}");
            assert!((y - 30.25).abs() < 0.1, "y {y}");
            assert!((width - 60.0).abs() < 0.1, "width {width}");
            assert!((height - 48.0).abs() < 0.1, "height {height}");
        }
        ref other => panic!("expected a rect, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// Corner preservation
// ---------------------------------------------------------------------------

#[test]
fn a_rotated_square_keeps_exactly_four_sharp_corners() {
    let angle = 0.35_f64;
    let centre = Point::new(90.0, 90.0);
    let half = 52.0;
    let corners: Vec<Point> = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|&(sx, sy)| Point::new(sx * half, sy * half).rotate(angle).add(centre))
        .collect();

    let image = render(
        180,
        180,
        &[Layer {
            color: [200, 120, 20, 255],
            inside: polygon(corners.clone()),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    assert_eq!(document.shapes.len(), 1);
    let segments = outer_segments(&document);
    assert_eq!(
        segments.len(),
        4,
        "a square should need four segments, got {segments:?}"
    );
    assert!(
        segments
            .iter()
            .all(|segment| matches!(segment, Segment::Line { .. })),
        "all four sides should be straight lines, got {segments:?}"
    );

    // Every true corner should have a recovered vertex essentially on top of it.
    let contour = document.shapes[0].outer.to_contour();
    let mut vertices = vec![contour.start];
    vertices.extend(contour.segments.iter().map(|segment| segment.end_point()));
    for corner in &corners {
        let nearest = vertices
            .iter()
            .map(|vertex| vertex.distance(*corner))
            .fold(f64::INFINITY, f64::min);
        assert!(
            nearest < 0.2,
            "corner {corner:?} recovered {nearest:.3}px away"
        );
    }
}

#[test]
fn a_star_keeps_all_ten_points_sharp() {
    // "Sharp logo marks" is the other failure case named in the README.
    let image = render(
        240,
        240,
        &[Layer {
            color: [240, 200, 30, 255],
            inside: star(120.0, 120.0, 105.0, 44.0, 5),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    assert_eq!(document.shapes.len(), 1, "a star is one shape");
    let segments = outer_segments(&document);
    assert_eq!(
        segments.len(),
        10,
        "a five-pointed star has ten straight sides, got {} segments",
        segments.len()
    );
    assert!(
        segments
            .iter()
            .all(|segment| matches!(segment, Segment::Line { .. })),
        "every side of a star is straight"
    );
}

#[test]
fn a_rounded_rectangle_keeps_round_corners_and_straight_sides() {
    let image = render(
        200,
        140,
        &[Layer {
            color: [40, 70, 160, 255],
            inside: rounded_rect(20.0, 20.0, 160.0, 100.0, 24.0),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    assert_eq!(document.shapes.len(), 1);
    let segments = outer_segments(&document);

    let lines = segments
        .iter()
        .filter(|segment| matches!(segment, Segment::Line { .. }))
        .count();
    let curves = segments.len() - lines;

    assert_eq!(lines, 4, "four straight sides, got {segments:?}");
    assert!(
        (4..=8).contains(&curves),
        "four rounded corners should need four to eight cubics, got {curves}"
    );
}

#[test]
fn a_triangle_needs_only_three_segments() {
    let vertices = vec![
        Point::new(100.0, 18.0),
        Point::new(178.0, 150.0),
        Point::new(22.0, 150.0),
    ];
    let image = render(
        200,
        170,
        &[Layer {
            color: [180, 30, 90, 255],
            inside: polygon(vertices.clone()),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    let segments = outer_segments(&document);
    assert_eq!(segments.len(), 3, "got {segments:?}");
}

// ---------------------------------------------------------------------------
// Accuracy and node economy
// ---------------------------------------------------------------------------

#[test]
fn a_multi_colour_mark_is_reproduced_accurately() {
    let image = render(
        220,
        220,
        &[
            Layer {
                color: [245, 245, 245, 255],
                inside: rounded_rect(10.0, 10.0, 200.0, 200.0, 36.0),
            },
            Layer {
                color: [200, 40, 50, 255],
                inside: disc(110.0, 110.0, 74.0),
            },
            Layer {
                color: [30, 60, 190, 255],
                inside: disc(110.0, 110.0, 38.0),
            },
        ],
    );
    let document = vectorize_image(&image, &logo_options());
    let score = accuracy(&document, &image);

    assert!(
        score > 0.99,
        "accuracy {score:.5} with stats {:?}",
        document.stats()
    );
    // The two discs should be recognised as circles.
    assert!(
        document.stats().circles >= 2,
        "expected the discs as circles, got {:?}",
        document.stats()
    );
}

#[test]
fn node_count_is_a_tiny_fraction_of_the_traced_outline() {
    let image = render(
        300,
        300,
        &[Layer {
            color: [20, 20, 20, 255],
            inside: rounded_rect(20.0, 20.0, 260.0, 260.0, 60.0),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    // The raw traced boundary is roughly one vertex per pixel of perimeter.
    let perimeter_estimate = 2.0 * (260.0 + 260.0);
    let nodes = document.stats().nodes;
    assert!(
        (nodes as f64) < perimeter_estimate / 40.0,
        "{nodes} nodes is too many for a rounded square"
    );

    let score = accuracy(&document, &image);
    assert!(score > 0.99, "accuracy {score:.5} with {nodes} nodes");
}

#[test]
fn tighter_tolerance_trades_nodes_for_accuracy() {
    let image = render(
        220,
        220,
        &[Layer {
            color: [90, 30, 140, 255],
            inside: star(110.0, 110.0, 95.0, 55.0, 7),
        }],
    );

    let loose = vectorize_image(
        &image,
        &VectorizeOptions {
            tolerance: 4.0,
            ..logo_options()
        },
    );
    let tight = vectorize_image(
        &image,
        &VectorizeOptions {
            tolerance: 0.4,
            ..logo_options()
        },
    );

    assert!(
        tight.stats().nodes >= loose.stats().nodes,
        "tight {:?} vs loose {:?}",
        tight.stats(),
        loose.stats()
    );

    // Accuracy is *not* strictly monotonic in tolerance, and should not be
    // asserted as such. Candidates are chosen by measurement, so a coarser ladder
    // can land on a candidate that happens to score a hair better than anything
    // the finer ladder offered. What matters is that tightening never costs real
    // accuracy.
    let tight_score = accuracy(&tight, &image);
    let loose_score = accuracy(&loose, &image);
    assert!(
        tight_score >= loose_score - 1e-4,
        "tightening lost accuracy: {tight_score:.6} vs {loose_score:.6}"
    );
    assert!(tight_score > 0.999, "tight accuracy was {tight_score:.6}");
}

// ---------------------------------------------------------------------------
// Robustness
// ---------------------------------------------------------------------------

#[test]
fn noisy_transparent_edges_still_recover_the_circle() {
    // The third README failure case. Deterministic noise is applied to both
    // colour and alpha, concentrated where it hurts: the anti-aliased rim.
    let mut image = render(
        200,
        200,
        &[Layer {
            color: [200, 60, 60, 255],
            inside: disc(100.0, 100.0, 68.0),
        }],
    );

    for y in 0..200u32 {
        for x in 0..200u32 {
            let pixel = image.get_pixel_mut(x, y);
            if pixel[3] == 0 {
                continue;
            }
            // Cheap deterministic hash, so the test never flakes.
            let hash = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) % 7;
            let jitter = hash as i32 - 3;
            for channel in 0..3 {
                pixel[channel] = (pixel[channel] as i32 + jitter).clamp(0, 255) as u8;
            }
            if pixel[3] < 255 {
                pixel[3] = (pixel[3] as i32 + jitter).clamp(1, 255) as u8;
            }
        }
    }

    let document = vectorize_image(&image, &logo_options());
    let (center, radius) =
        find_circle(&document).expect("noise should not stop a circle being recognised");
    assert!(
        center.distance(Point::new(100.0, 100.0)) < 0.25,
        "centre drifted to {center:?}"
    );
    assert!(
        (radius - 68.0).abs() < 0.25,
        "radius recovered as {radius}, expected 68"
    );
}

#[test]
fn a_small_icon_still_recovers_its_geometry() {
    // 24px icons are where sub-pixel accuracy matters most: half a pixel is
    // 4% of the whole glyph.
    let image = render(
        24,
        24,
        &[Layer {
            color: [0, 0, 0, 255],
            inside: disc(12.0, 12.0, 9.0),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    let (center, radius) = find_circle(&document).expect("a small disc is still a circle");
    assert!(center.distance(Point::new(12.0, 12.0)) < 0.15, "{center:?}");
    assert!((radius - 9.0).abs() < 0.15, "radius {radius}");
}

#[test]
fn hard_edged_pixel_art_is_reproduced_exactly() {
    let image = RgbaImage::from_fn(12, 12, |x, y| {
        if (x + y) % 3 == 0 {
            Rgba([230, 40, 40, 255])
        } else if x % 4 == 0 {
            Rgba([40, 40, 230, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    });

    let document = vectorize_image(
        &image,
        &VectorizeOptions {
            mode: VectorizeMode::PixelArt,
            colors: 4,
            detail: 1.0,
            smoothness: 0.0,
            tolerance: 0.5,
        },
    );

    // Crisp mode must place every edge exactly on the pixel grid.
    for shape in &document.shapes {
        let contour = shape.outer.to_contour();
        for point in contour.flatten(0.01) {
            assert!(
                (point.x.round() - point.x).abs() < 1e-9
                    && (point.y.round() - point.y).abs() < 1e-9,
                "pixel art vertex left the grid at {point:?}"
            );
        }
    }

    let score = accuracy(&document, &image);
    assert!(score > 0.999, "pixel art accuracy was {score:.5}");
}

#[test]
fn a_shape_running_off_the_canvas_is_clipped_cleanly() {
    let image = render(
        100,
        100,
        &[Layer {
            color: [60, 160, 60, 255],
            inside: rect(-20.0, -20.0, 80.0, 80.0),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    assert_eq!(document.shapes.len(), 1);
    let bounds = document.shapes[0].outer.bounds();
    assert!(bounds.min_x.abs() < 0.1, "{bounds:?}");
    assert!(bounds.min_y.abs() < 0.1, "{bounds:?}");
    assert!((bounds.max_x - 60.0).abs() < 0.1, "{bounds:?}");
    assert!((bounds.max_y - 60.0).abs() < 0.1, "{bounds:?}");
}

#[test]
fn semi_transparent_fills_keep_their_alpha() {
    let image = render(
        120,
        120,
        &[Layer {
            color: [200, 40, 40, 128],
            inside: disc(60.0, 60.0, 40.0),
        }],
    );
    let document = vectorize_image(&image, &logo_options());

    assert_eq!(document.shapes.len(), 1);
    let alpha = document.shapes[0].color[3];
    assert!(
        (alpha as i32 - 128).abs() <= 2,
        "alpha recovered as {alpha}, expected 128"
    );
    assert!(svg_contains(&document, "fill-opacity"));
}

fn svg_contains(document: &Document, needle: &str) -> bool {
    document.to_svg().contains(needle)
}
