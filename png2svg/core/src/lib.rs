use std::collections::HashMap;
use std::fmt::Write as FmtWrite;

use image::{Rgba, RgbaImage};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum VectorizeError {
    #[error("failed to decode image: {0}")]
    Decode(#[from] image::ImageError),
    #[error("vectorization failed: {0}")]
    Vectorize(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum VectorizeMode {
    Logo,
    Poster,
    PixelArt,
}

impl Default for VectorizeMode {
    fn default() -> Self {
        Self::Logo
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VectorizeOptions {
    pub colors: u8,
    pub detail: f32,
    pub smoothness: f32,
    pub mode: VectorizeMode,
}

impl Default for VectorizeOptions {
    fn default() -> Self {
        Self {
            colors: 8,
            detail: 0.6,
            smoothness: 0.5,
            mode: VectorizeMode::Logo,
        }
    }
}

pub fn png_to_svg(png_bytes: &[u8], options: &VectorizeOptions) -> Result<String, VectorizeError> {
    let image = image::load_from_memory(png_bytes)?;
    let rgba = image.to_rgba8();

    let palette_size = palette_size_from_options(options);
    let palette = build_palette(&rgba, palette_size);
    let indexed = map_to_palette(&rgba, &palette);
    let svg = render_svg(&indexed, &palette, rgba.width(), rgba.height(), options);

    Ok(svg)
}

fn palette_size_from_options(options: &VectorizeOptions) -> usize {
    let clamped_detail = options.detail.clamp(0.1, 1.0);
    let base = options.colors.max(2) as f32;
    (base * clamped_detail).ceil() as usize
}

fn build_palette(image: &RgbaImage, max_colors: usize) -> Vec<[u8; 4]> {
    let mut histogram: HashMap<[u8; 4], u32> = HashMap::new();

    for pixel in image.pixels() {
        if pixel[3] == 0 {
            continue;
        }
        *histogram.entry(pixel.0).or_insert(0) += 1;
    }

    let mut entries: Vec<([u8; 4], u32)> = histogram.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1));

    let mut palette: Vec<[u8; 4]> = entries
        .into_iter()
        .take(max_colors.max(1))
        .map(|(color, _)| color)
        .collect();

    if palette.is_empty() {
        palette.push([0, 0, 0, 0]);
    }

    palette
}

fn map_to_palette(image: &RgbaImage, palette: &[[u8; 4]]) -> Vec<usize> {
    image
        .pixels()
        .map(|pixel| nearest_palette_index(pixel, palette))
        .collect()
}

fn nearest_palette_index(pixel: &Rgba<u8>, palette: &[[u8; 4]]) -> usize {
    let mut best = 0;
    let mut best_dist = u32::MAX;

    for (idx, color) in palette.iter().enumerate() {
        let dist = color_distance(pixel.0, *color);
        if dist < best_dist {
            best = idx;
            best_dist = dist;
        }
    }

    best
}

fn color_distance(a: [u8; 4], b: [u8; 4]) -> u32 {
    let dr = a[0] as i32 - b[0] as i32;
    let dg = a[1] as i32 - b[1] as i32;
    let db = a[2] as i32 - b[2] as i32;
    let da = a[3] as i32 - b[3] as i32;
    (dr * dr + dg * dg + db * db + da * da) as u32
}

fn render_svg(
    indexed: &[usize],
    palette: &[[u8; 4]],
    width: u32,
    height: u32,
    options: &VectorizeOptions,
) -> String {
    let mut svg = String::with_capacity((width * height) as usize);
    writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" aria-label=\"vectorized\" shape-rendering=\"crispEdges\">",
        w = width,
        h = height
    )
    .ok();

    for y in 0..height as usize {
        let row_offset = y * width as usize;
        let mut x = 0;
        while x < width as usize {
            let color_index = indexed[row_offset + x];
            let color = palette[color_index];
            let mut run_end = x + 1;
            while run_end < width as usize && indexed[row_offset + run_end] == color_index {
                run_end += 1;
            }

            if color[3] > 0 {
                let opacity = opacity_from_options(color[3], options);
                let hex = to_hex(color);
                writeln!(
                    svg,
                    "  <rect x=\"{x}\" y=\"{y}\" width=\"{w}\" height=\"1\" fill=\"#{hex}\" fill-opacity=\"{opacity:.3}\" />",
                    x = x,
                    y = y,
                    w = run_end - x,
                    hex = hex,
                    opacity = opacity
                )
                .ok();
            }

            x = run_end;
        }
    }

    svg.push_str("</svg>");
    svg
}

fn opacity_from_options(alpha: u8, options: &VectorizeOptions) -> f32 {
    let smoothness = options.smoothness.clamp(0.2, 1.0);
    let base = alpha as f32 / 255.0;
    (base * smoothness).clamp(0.05, 1.0)
}

fn to_hex(color: [u8; 4]) -> String {
    let mut s = String::with_capacity(6);
    write!(&mut s, "{:02x}{:02x}{:02x}", color[0], color[1], color[2]).ok();
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::png::PngEncoder, ColorType, DynamicImage, ImageEncoder};

    #[test]
    fn creates_svg_output() {
        let image = RgbaImage::from_fn(2, 2, |x, y| {
            let alpha = if (x + y) % 2 == 0 { 255 } else { 128 };
            Rgba([x as u8 * 80, y as u8 * 40, 200, alpha])
        });

        let mut png_bytes = Vec::new();
        PngEncoder::new(&mut png_bytes)
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                ColorType::Rgba8.into(),
            )
            .expect("image should encode to png");

        let options = VectorizeOptions::default();
        let svg = png_to_svg(&png_bytes, &options).expect("svg generation should succeed");

        assert!(svg.contains("<svg"));
        assert!(svg.contains("rect"));
    }

    #[test]
    fn respects_palette_size() {
        let image = DynamicImage::new_rgba8(4, 4).to_rgba8();
        let palette = build_palette(&image, 4);
        assert_eq!(palette.len(), 1, "empty images fall back to one color");

        let non_empty = RgbaImage::from_fn(4, 4, |x, y| {
            let alpha = if (x + y) % 2 == 0 { 255 } else { 128 };
            Rgba([x as u8 * 10, y as u8 * 10, 50, alpha])
        });
        let palette = build_palette(&non_empty, 3);
        assert!(palette.len() <= 3);
    }
}
