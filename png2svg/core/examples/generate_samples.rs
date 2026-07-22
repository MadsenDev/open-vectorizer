use std::fs;
use std::path::Path;

use image::{codecs::png::PngEncoder, ColorType, ImageEncoder, Rgba, RgbaImage};
use png2svg_core::{png_to_svg, VectorizeMode, VectorizeOptions};

const SAMPLE_SIZE: u32 = 128;
const SUPERSAMPLE: u32 = 4;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = Path::new("target/vectorizer-samples");
    let input_dir = output_dir.join("inputs");
    let svg_dir = output_dir.join("svg");
    fs::create_dir_all(&input_dir)?;
    fs::create_dir_all(&svg_dir)?;

    for sample in samples() {
        let png = encode_png(&sample.image)?;
        fs::write(input_dir.join(format!("{}.png", sample.name)), &png)?;

        let svg = png_to_svg(&png, &sample.options)?;
        fs::write(svg_dir.join(format!("{}.svg", sample.name)), svg)?;
    }

    Ok(())
}

struct Sample {
    name: &'static str,
    image: RgbaImage,
    options: VectorizeOptions,
}

fn samples() -> Vec<Sample> {
    vec![
        Sample {
            name: "smooth-ring-badge",
            image: smooth_ring_badge(),
            options: VectorizeOptions::default(),
        },
        Sample {
            name: "vardir-v-mark",
            image: vardir_v_mark(),
            options: VectorizeOptions::default(),
        },
        Sample {
            name: "transparent-compass-logo",
            image: transparent_compass_logo(),
            options: VectorizeOptions::default(),
        },
        Sample {
            name: "noisy-logo-cleanup",
            image: noisy_logo_cleanup(),
            options: VectorizeOptions::default(),
        },
        Sample {
            name: "pixel-diagonal",
            image: pixel_diagonal(),
            options: VectorizeOptions {
                colors: 2,
                mode: VectorizeMode::PixelArt,
                ..VectorizeOptions::default()
            },
        },
    ]
}

fn encode_png(image: &RgbaImage) -> Result<Vec<u8>, image::ImageError> {
    let mut png_bytes = Vec::new();
    PngEncoder::new(&mut png_bytes).write_image(
        image.as_raw(),
        image.width(),
        image.height(),
        ColorType::Rgba8.into(),
    )?;
    Ok(png_bytes)
}

fn smooth_ring_badge() -> RgbaImage {
    supersampled_logo(SAMPLE_SIZE, SAMPLE_SIZE, |x, y| {
        let d = distance(x, y, 64.0, 64.0);
        if (28.0..=50.0).contains(&d) {
            Rgba([24, 27, 32, 255])
        } else {
            transparent()
        }
    })
}

fn vardir_v_mark() -> RgbaImage {
    let left = [(28.0, 22.0), (50.0, 22.0), (66.0, 92.0), (54.0, 112.0)];
    let right = [(78.0, 22.0), (100.0, 22.0), (74.0, 112.0), (62.0, 92.0)];

    supersampled_logo(SAMPLE_SIZE, SAMPLE_SIZE, |x, y| {
        if point_in_polygon(x, y, &left) || point_in_polygon(x, y, &right) {
            Rgba([16, 120, 105, 255])
        } else {
            transparent()
        }
    })
}

fn transparent_compass_logo() -> RgbaImage {
    let needle = [(64.0, 20.0), (78.0, 66.0), (64.0, 108.0), (50.0, 66.0)];

    supersampled_logo(SAMPLE_SIZE, SAMPLE_SIZE, |x, y| {
        let d = distance(x, y, 64.0, 64.0);
        if d <= 46.0 && !point_in_polygon(x, y, &needle) {
            Rgba([28, 31, 38, 255])
        } else if point_in_polygon(x, y, &needle) {
            Rgba([235, 180, 64, 255])
        } else {
            transparent()
        }
    })
}

fn noisy_logo_cleanup() -> RgbaImage {
    supersampled_logo(SAMPLE_SIZE, SAMPLE_SIZE, |x, y| {
        let in_mark = rounded_rect(x, y, 30.0, 38.0, 98.0, 90.0, 14.0);
        let speck = distance(x, y, 15.0, 15.0) < 1.5
            || distance(x, y, 112.0, 20.0) < 1.5
            || distance(x, y, 18.0, 112.0) < 1.5;

        if in_mark || speck {
            Rgba([32, 35, 42, 255])
        } else {
            transparent()
        }
    })
}

fn pixel_diagonal() -> RgbaImage {
    RgbaImage::from_fn(12, 12, |x, y| {
        if x == y || x + 1 == y {
            Rgba([20, 20, 20, 255])
        } else {
            transparent()
        }
    })
}

fn supersampled_logo(
    width: u32,
    height: u32,
    sample: impl Fn(f32, f32) -> Rgba<u8>,
) -> RgbaImage {
    RgbaImage::from_fn(width, height, |x, y| {
        let mut rgb = [0u32; 3];
        let mut alpha = 0u32;
        let mut covered = 0u32;
        for sy in 0..SUPERSAMPLE {
            for sx in 0..SUPERSAMPLE {
                let px = x as f32 + (sx as f32 + 0.5) / SUPERSAMPLE as f32;
                let py = y as f32 + (sy as f32 + 0.5) / SUPERSAMPLE as f32;
                let sample = sample(px, py).0;
                alpha += sample[3] as u32;
                if sample[3] > 0 {
                    covered += 1;
                    for channel in 0..3 {
                        rgb[channel] += sample[channel] as u32;
                    }
                }
            }
        }

        let samples = SUPERSAMPLE * SUPERSAMPLE;
        if covered == 0 {
            return transparent();
        }

        Rgba([
            (rgb[0] / covered) as u8,
            (rgb[1] / covered) as u8,
            (rgb[2] / covered) as u8,
            (alpha / samples) as u8,
        ])
    })
}

fn transparent() -> Rgba<u8> {
    Rgba([0, 0, 0, 0])
}

fn distance(x: f32, y: f32, cx: f32, cy: f32) -> f32 {
    let dx = x - cx;
    let dy = y - cy;
    (dx * dx + dy * dy).sqrt()
}

fn rounded_rect(x: f32, y: f32, left: f32, top: f32, right: f32, bottom: f32, radius: f32) -> bool {
    let cx = x.clamp(left + radius, right - radius);
    let cy = y.clamp(top + radius, bottom - radius);
    distance(x, y, cx, cy) <= radius
}

fn point_in_polygon(x: f32, y: f32, points: &[(f32, f32)]) -> bool {
    let mut inside = false;
    let mut j = points.len() - 1;

    for i in 0..points.len() {
        let (xi, yi) = points[i];
        let (xj, yj) = points[j];
        let crosses = (yi > y) != (yj > y);
        if crosses && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
        j = i;
    }

    inside
}
