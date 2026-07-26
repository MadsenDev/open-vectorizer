//! Ground-truth benchmark for the vectorizer.
//!
//! Generates shapes whose exact geometry is known, renders them with proper
//! anti-aliasing, vectorizes the pixels, and reports how much of the original
//! geometry came back. Because the source geometry is known, the numbers are
//! measurements rather than impressions.
//!
//! Run with:
//!
//! ```text
//! cargo run --release -p png2svg-core --example benchmark
//! ```
//!
//! This is also the harness for comparing engines. Any other vectorizer that
//! reads a PNG and writes an SVG can be scored on the same inputs; the columns
//! that matter are `nodes` (how much geometry it took) and `accuracy` (how well
//! that geometry reproduces the input). A tool that wins on accuracy while
//! emitting ten times the nodes has not won.

use std::time::Instant;

use image::{Rgba, RgbaImage};
use png2svg_core::geom::Point;
use png2svg_core::path::Outline;
use png2svg_core::svg::Document;
use png2svg_core::{accuracy, vectorize_image, VectorizeMode, VectorizeOptions};

const SUPERSAMPLE: usize = 16;

type InsideTest = Box<dyn Fn(f64, f64) -> bool>;

struct Layer {
    color: [u8; 4],
    inside: InsideTest,
}

struct Case {
    name: &'static str,
    size: (u32, u32),
    layers: Vec<Layer>,
    /// Ground-truth circle to check recovery against, when the case has one.
    expected_circle: Option<(Point, f64)>,
    mode: VectorizeMode,
}

fn main() {
    println!(
        "{:<26} {:>9} {:>8} {:>7} {:>7} {:>10} {:>9} {:>10}",
        "case", "size", "time", "shapes", "nodes", "primitives", "accuracy", "geom err"
    );
    println!("{}", "-".repeat(92));

    let mut worst_accuracy = 1.0f64;
    let mut worst_geometry = 0.0f64;

    for case in cases() {
        let image = render(case.size.0, case.size.1, &case.layers);
        let options = VectorizeOptions {
            mode: case.mode,
            ..VectorizeOptions::default()
        };

        let started = Instant::now();
        let document = vectorize_image(&image, &options);
        let elapsed = started.elapsed().as_secs_f64() * 1000.0;

        let stats = document.stats();
        let score = accuracy(&document, &image);
        worst_accuracy = worst_accuracy.min(score);

        let geometry_error = case
            .expected_circle
            .map(|(center, radius)| circle_error(&document, center, radius));
        if let Some(error) = geometry_error {
            worst_geometry = worst_geometry.max(error);
        }

        println!(
            "{:<26} {:>9} {:>7.1}ms {:>7} {:>7} {:>10} {:>9.5} {:>10}",
            case.name,
            format!("{}x{}", case.size.0, case.size.1),
            elapsed,
            stats.shapes,
            stats.nodes,
            format!(
                "{}c {}e {}r",
                stats.circles, stats.ellipses, stats.rects
            ),
            score,
            geometry_error
                .map(|error| format!("{error:.4}px"))
                .unwrap_or_else(|| "-".to_string()),
        );
    }

    println!("{}", "-".repeat(92));
    println!("worst accuracy: {worst_accuracy:.5}");
    println!("worst circle recovery error: {worst_geometry:.4}px");
}

/// Largest distance between a recovered circle and the true one, over centre and
/// radius. Reports a large number when no circle was recovered at all, since
/// that is a failure of the same kind.
fn circle_error(document: &Document, center: Point, radius: f64) -> f64 {
    let mut best = f64::INFINITY;
    for shape in &document.shapes {
        for outline in std::iter::once(&shape.outer).chain(shape.holes.iter()) {
            if let Outline::Circle {
                center: found,
                radius: found_radius,
            } = outline
            {
                let error = found.distance(center).max((found_radius - radius).abs());
                best = best.min(error);
            }
        }
    }
    if best.is_finite() {
        best
    } else {
        f64::from(u8::MAX)
    }
}

fn render(width: u32, height: u32, layers: &[Layer]) -> RgbaImage {
    let mut image = RgbaImage::new(width, height);
    let step = 1.0 / SUPERSAMPLE as f64;

    for y in 0..height {
        for x in 0..width {
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
                    to_byte(accumulated[0] / samples / alpha),
                    to_byte(accumulated[1] / samples / alpha),
                    to_byte(accumulated[2] / samples / alpha),
                    to_byte(alpha),
                ])
            };
            image.put_pixel(x, y, pixel);
        }
    }

    image
}

fn to_byte(value: f64) -> u8 {
    (value * 255.0).round().clamp(0.0, 255.0) as u8
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

fn ellipse(cx: f64, cy: f64, rx: f64, ry: f64) -> InsideTest {
    Box::new(move |x, y| ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0)
}

fn polygon(vertices: Vec<Point>) -> InsideTest {
    Box::new(move |x, y| png2svg_core::geom::point_in_polygon(Point::new(x, y), &vertices))
}

fn rounded_rect(x0: f64, y0: f64, w: f64, h: f64, radius: f64) -> InsideTest {
    Box::new(move |x, y| {
        if x < x0 || y < y0 || x >= x0 + w || y >= y0 + h {
            return false;
        }
        let cx = x.clamp(x0 + radius, x0 + w - radius);
        let cy = y.clamp(y0 + radius, y0 + h - radius);
        (x - cx).powi(2) + (y - cy).powi(2) <= radius * radius
    })
}

fn star(cx: f64, cy: f64, outer: f64, inner: f64, points: usize) -> InsideTest {
    let vertices = (0..points * 2)
        .map(|index| {
            let radius = if index % 2 == 0 { outer } else { inner };
            let angle = index as f64 / (points * 2) as f64 * std::f64::consts::TAU
                - std::f64::consts::FRAC_PI_2;
            Point::new(cx + radius * angle.cos(), cy + radius * angle.sin())
        })
        .collect();
    polygon(vertices)
}

fn rotated_square(cx: f64, cy: f64, half: f64, angle: f64) -> InsideTest {
    let centre = Point::new(cx, cy);
    let vertices = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .iter()
        .map(|&(sx, sy)| Point::new(sx * half, sy * half).rotate(angle).add(centre))
        .collect();
    polygon(vertices)
}

fn ink(color: [u8; 4], inside: InsideTest) -> Layer {
    Layer { color, inside }
}

fn cases() -> Vec<Case> {
    let dark = [24, 28, 36, 255];
    let red = [200, 45, 50, 255];
    let blue = [35, 70, 190, 255];
    let gold = [240, 195, 45, 255];
    let paper = [246, 246, 244, 255];

    vec![
        Case {
            name: "circle",
            size: (200, 200),
            layers: vec![ink(red, disc(100.0, 100.0, 70.0))],
            expected_circle: Some((Point::new(100.0, 100.0), 70.0)),
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "circle off-grid",
            size: (160, 160),
            layers: vec![ink(blue, disc(80.37, 79.62, 51.45))],
            expected_circle: Some((Point::new(80.37, 79.62), 51.45)),
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "ring",
            size: (200, 200),
            layers: vec![ink(dark, ring(100.0, 100.0, 74.0, 46.0))],
            expected_circle: Some((Point::new(100.0, 100.0), 74.0)),
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "small icon",
            size: (24, 24),
            layers: vec![ink(dark, disc(12.0, 12.0, 9.0))],
            expected_circle: Some((Point::new(12.0, 12.0), 9.0)),
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "ellipse",
            size: (220, 160),
            layers: vec![ink(gold, ellipse(110.0, 80.0, 90.0, 50.0))],
            expected_circle: None,
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "square",
            size: (120, 120),
            layers: vec![ink(dark, rect(24.5, 30.25, 60.0, 48.0))],
            expected_circle: None,
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "rotated square",
            size: (180, 180),
            layers: vec![ink(red, rotated_square(90.0, 90.0, 52.0, 0.35))],
            expected_circle: None,
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "triangle",
            size: (200, 170),
            layers: vec![ink(blue, polygon(vec![
                Point::new(100.0, 18.0),
                Point::new(178.0, 150.0),
                Point::new(22.0, 150.0),
            ]))],
            expected_circle: None,
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "5-point star",
            size: (240, 240),
            layers: vec![ink(gold, star(120.0, 120.0, 105.0, 44.0, 5))],
            expected_circle: None,
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "rounded rect",
            size: (200, 140),
            layers: vec![ink(blue, rounded_rect(20.0, 20.0, 160.0, 100.0, 24.0))],
            expected_circle: None,
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "capsule",
            size: (220, 100),
            layers: vec![ink(dark, rounded_rect(15.0, 25.0, 190.0, 50.0, 25.0))],
            expected_circle: None,
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "badge (3 colours)",
            size: (220, 220),
            layers: vec![
                ink(paper, rounded_rect(10.0, 10.0, 200.0, 200.0, 36.0)),
                ink(red, disc(110.0, 110.0, 74.0)),
                ink(blue, disc(110.0, 110.0, 38.0)),
            ],
            expected_circle: Some((Point::new(110.0, 110.0), 74.0)),
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "wordmark-ish bars",
            size: (240, 120),
            layers: vec![
                ink(dark, rect(20.0, 30.0, 30.0, 60.0)),
                ink(dark, rect(70.0, 30.0, 30.0, 60.0)),
                ink(dark, rect(120.0, 30.0, 30.0, 60.0)),
                ink(red, disc(190.0, 60.0, 26.0)),
            ],
            expected_circle: Some((Point::new(190.0, 60.0), 26.0)),
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "circle @ 1024px",
            size: (1024, 1024),
            layers: vec![ink(red, disc(512.0, 512.0, 400.0))],
            expected_circle: Some((Point::new(512.0, 512.0), 400.0)),
            mode: VectorizeMode::Logo,
        },
        Case {
            name: "star @ 1024px",
            size: (1024, 1024),
            layers: vec![ink(gold, star(512.0, 512.0, 460.0, 190.0, 7))],
            expected_circle: None,
            mode: VectorizeMode::Logo,
        },
    ]
}
