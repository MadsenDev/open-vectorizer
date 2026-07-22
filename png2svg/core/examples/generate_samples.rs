use std::fs;
use std::path::Path;

use image::{codecs::png::PngEncoder, ColorType, ImageEncoder, Rgba, RgbaImage};
use png2svg_core::{png_to_svg, VectorizeMode, VectorizeOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = Path::new("target/vectorizer-samples");
    fs::create_dir_all(output_dir)?;

    for sample in samples() {
        let svg = png_to_svg(&encode_png(&sample.image)?, &sample.options)?;
        fs::write(output_dir.join(format!("{}.svg", sample.name)), svg)?;
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
            name: "transparent-ring-logo",
            image: transparent_ring_logo(),
            options: VectorizeOptions {
                colors: 2,
                mode: VectorizeMode::Logo,
                ..VectorizeOptions::default()
            },
        },
        Sample {
            name: "two-color-mark",
            image: two_color_mark(),
            options: VectorizeOptions {
                colors: 3,
                detail: 0.8,
                mode: VectorizeMode::Logo,
                ..VectorizeOptions::default()
            },
        },
        Sample {
            name: "speckled-logo-cleanup",
            image: speckled_logo(),
            options: VectorizeOptions {
                colors: 3,
                mode: VectorizeMode::Logo,
                ..VectorizeOptions::default()
            },
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

fn transparent_ring_logo() -> RgbaImage {
    RgbaImage::from_fn(32, 32, |x, y| {
        let dx = x as i32 - 16;
        let dy = y as i32 - 16;
        let dist_sq = dx * dx + dy * dy;
        if (70..=190).contains(&dist_sq) {
            Rgba([30, 30, 34, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

fn two_color_mark() -> RgbaImage {
    RgbaImage::from_fn(36, 24, |x, y| {
        let left_block = (4..=17).contains(&x) && (5..=18).contains(&y);
        let right_block = (18..=31).contains(&x) && (5..=18).contains(&y);
        if left_block {
            Rgba([220, 40, 40, 255])
        } else if right_block {
            Rgba([40, 90, 220, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

fn speckled_logo() -> RgbaImage {
    RgbaImage::from_fn(32, 32, |x, y| {
        let mark = (7..=24).contains(&x) && (9..=22).contains(&y);
        let speck = matches!((x, y), (2, 2) | (28, 4) | (4, 27));
        if mark || speck {
            Rgba([18, 18, 20, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

fn pixel_diagonal() -> RgbaImage {
    RgbaImage::from_fn(12, 12, |x, y| {
        if x == y || x + 1 == y {
            Rgba([20, 20, 20, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}
