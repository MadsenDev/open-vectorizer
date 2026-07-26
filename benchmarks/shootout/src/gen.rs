//! Generate the shootout cases: an anti-aliased PNG per case, a thresholded PBM
//! for potrace (which is a 1-bit tracer and cannot read the PNG), and our own
//! SVG output.

use std::fs;
use std::io::Write;
use std::path::Path;

use image::{Rgba, RgbaImage};
use png2svg_core::geom::Point;
use png2svg_core::{vectorize_image, VectorizeMode, VectorizeOptions};

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
    /// Single-colour cases can be compared on coverage alone, which is what
    /// makes potrace comparable at all.
    single_colour: bool,
}

fn main() {
    let out = Path::new(&std::env::args().nth(1).expect("output dir")).to_path_buf();
    fs::create_dir_all(&out).unwrap();

    let mut manifest = String::new();

    for case in cases() {
        let image = render(case.size.0, case.size.1, &case.layers);
        let png_path = out.join(format!("{}.png", case.name));
        image.save(&png_path).unwrap();

        // An opaque variant on white. VTracer is built for opaque input and
        // traces the transparent region as a shape otherwise, so this is the
        // input that gives it a fair run - and "logo on white" is the commonest
        // real case anyway.
        let mut white = image.clone();
        for pixel in white.pixels_mut() {
            let alpha = pixel[3] as f64 / 255.0;
            for channel in 0..3 {
                let over = pixel[channel] as f64 / 255.0 * alpha + (1.0 - alpha);
                pixel[channel] = byte(over);
            }
            pixel[3] = 255;
        }
        white.save(out.join(format!("{}.white.png", case.name))).unwrap();

        // Bilevel PBM for potrace: threshold at half coverage, which is the
        // standard way anti-aliased art is fed to a 1-bit tracer.
        let mut pbm = format!("P1\n{} {}\n", case.size.0, case.size.1);
        for y in 0..case.size.1 {
            for x in 0..case.size.0 {
                let alpha = image.get_pixel(x, y)[3];
                pbm.push(if alpha >= 128 { '1' } else { '0' });
                pbm.push(' ');
            }
            pbm.push('\n');
        }
        let mut file = fs::File::create(out.join(format!("{}.pbm", case.name))).unwrap();
        file.write_all(pbm.as_bytes()).unwrap();

        // Our own output, default options in logo mode.
        let document = vectorize_image(
            &image,
            &VectorizeOptions {
                mode: VectorizeMode::Logo,
                ..VectorizeOptions::default()
            },
        );
        fs::write(
            out.join(format!("{}.ours.svg", case.name)),
            document.to_svg(),
        )
        .unwrap();

        manifest.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            case.name,
            case.size.0,
            case.size.1,
            if case.single_colour { "mono" } else { "colour" }
        ));
    }

    fs::write(out.join("manifest.tsv"), manifest).unwrap();
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
                    byte(accumulated[0] / samples / alpha),
                    byte(accumulated[1] / samples / alpha),
                    byte(accumulated[2] / samples / alpha),
                    byte(alpha),
                ])
            };
            image.put_pixel(x, y, pixel);
        }
    }
    image
}

fn byte(value: f64) -> u8 {
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

/// A smooth organic blob, for a case with no primitives and no corners.
fn blob(cx: f64, cy: f64, base: f64) -> InsideTest {
    Box::new(move |x, y| {
        let dx = x - cx;
        let dy = y - cy;
        let angle = dy.atan2(dx);
        let radius = base * (1.0 + 0.28 * (3.0 * angle).sin() + 0.12 * (5.0 * angle).cos());
        (dx * dx + dy * dy).sqrt() <= radius
    })
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

    let mono = |name: &'static str, size: (u32, u32), inside: InsideTest| Case {
        name,
        size,
        layers: vec![ink(dark, inside)],
        single_colour: true,
    };

    vec![
        mono("circle", (200, 200), disc(100.0, 100.0, 70.0)),
        mono("circle-offgrid", (160, 160), disc(80.37, 79.62, 51.45)),
        mono("ring", (200, 200), ring(100.0, 100.0, 74.0, 46.0)),
        mono("ellipse", (220, 160), ellipse(110.0, 80.0, 90.0, 50.0)),
        mono("square", (120, 120), rect(24.5, 30.25, 60.0, 48.0)),
        mono(
            "rotated-square",
            (180, 180),
            rotated_square(90.0, 90.0, 52.0, 0.35),
        ),
        mono(
            "triangle",
            (200, 170),
            polygon(vec![
                Point::new(100.0, 18.0),
                Point::new(178.0, 150.0),
                Point::new(22.0, 150.0),
            ]),
        ),
        mono("star5", (240, 240), star(120.0, 120.0, 105.0, 44.0, 5)),
        mono(
            "rounded-rect",
            (200, 140),
            rounded_rect(20.0, 20.0, 160.0, 100.0, 24.0),
        ),
        mono(
            "capsule",
            (220, 100),
            rounded_rect(15.0, 25.0, 190.0, 50.0, 25.0),
        ),
        mono("icon24", (24, 24), disc(12.0, 12.0, 9.0)),
        mono("blob", (240, 240), blob(120.0, 120.0, 82.0)),
        mono(
            "thick-L",
            (180, 200),
            polygon(vec![
                Point::new(30.0, 20.0),
                Point::new(75.0, 20.0),
                Point::new(75.0, 135.0),
                Point::new(155.0, 135.0),
                Point::new(155.0, 180.0),
                Point::new(30.0, 180.0),
            ]),
        ),
        mono("circle-1024", (1024, 1024), disc(512.0, 512.0, 400.0)),
        Case {
            name: "badge",
            size: (220, 220),
            layers: vec![
                ink(paper, rounded_rect(10.0, 10.0, 200.0, 200.0, 36.0)),
                ink(red, disc(110.0, 110.0, 74.0)),
                ink(blue, disc(110.0, 110.0, 38.0)),
            ],
            single_colour: false,
        },
        Case {
            name: "wordmark",
            size: (240, 120),
            layers: vec![
                ink(dark, rect(20.0, 30.0, 30.0, 60.0)),
                ink(dark, rect(70.0, 30.0, 30.0, 60.0)),
                ink(dark, rect(120.0, 30.0, 30.0, 60.0)),
                ink(red, disc(190.0, 60.0, 26.0)),
            ],
            single_colour: false,
        },
        Case {
            name: "three-colour-mark",
            size: (200, 200),
            layers: vec![
                ink(gold, star(100.0, 100.0, 88.0, 40.0, 6)),
                ink(red, disc(100.0, 100.0, 46.0)),
                ink(blue, rect(85.0, 30.0, 30.0, 140.0)),
            ],
            single_colour: false,
        },
    ]
}
