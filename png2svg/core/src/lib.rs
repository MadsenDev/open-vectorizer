//! Open Vectorizer core engine.
//!
//! Converts raster logos, icons and flat artwork into clean SVG. The engine is
//! deterministic and entirely classical — no model, no inference, no network.
//!
//! The pipeline treats an anti-aliased pixel as a *measurement of coverage*
//! rather than as a color needing a palette slot. That single change is what
//! lets boundaries be placed to a fraction of a pixel, which in turn makes
//! corner recovery and primitive detection possible. Candidate geometry is then
//! rendered back and scored against the source coverage, so the accuracy versus
//! node-count trade-off is decided by measurement instead of by tuning.
//!
//! See [`vectorize`] for the stage-by-stage overview.
//!
//! ```no_run
//! use png2svg_core::{png_to_svg, VectorizeOptions};
//!
//! let bytes = std::fs::read("logo.png").unwrap();
//! let svg = png_to_svg(&bytes, &VectorizeOptions::default()).unwrap();
//! ```

pub mod corner;
pub mod field;
pub mod fit;
pub mod geom;
pub mod path;
pub mod primitive;
pub mod quantize;
pub mod raster;
pub mod svg;
pub mod trace;
pub mod vectorize;

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

pub use svg::{Document, Stats};
pub use vectorize::{accuracy, resolve_auto_options};

#[derive(Debug, Error)]
pub enum VectorizeError {
    #[error("failed to decode image: {0}")]
    Decode(#[from] image::ImageError),
    #[error("vectorization failed: {0}")]
    Vectorize(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
#[derive(Default)]
pub enum VectorizeMode {
    #[default]
    Auto,
    Logo,
    Poster,
    #[serde(rename = "pixel", alias = "pixelart", alias = "pixel-art")]
    PixelArt,
}


#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VectorizeOptions {
    /// Palette size ceiling. In `Auto` mode this is inferred from the image.
    pub colors: u8,
    /// How much small structure to keep, `0.0..=1.0`. Drives speck removal.
    pub detail: f32,
    /// How much evidence is needed before a curve is broken by a corner,
    /// `0.0..=1.0`.
    pub smoothness: f32,
    /// Geometric error ceiling. A quarter of this value is the budget in
    /// pixels, so the default 1.5 allows about 0.38px.
    pub tolerance: f32,
    pub mode: VectorizeMode,
}

impl Default for VectorizeOptions {
    fn default() -> Self {
        Self {
            colors: 8,
            detail: 0.6,
            smoothness: 0.5,
            tolerance: 1.5,
            mode: VectorizeMode::Auto,
        }
    }
}

/// Vectorize an encoded image (PNG, JPEG, GIF, BMP, ...) to an SVG string.
///
/// Requires the `decode` feature, which is on by default.
#[cfg(feature = "decode")]
pub fn png_to_svg(bytes: &[u8], options: &VectorizeOptions) -> Result<String, VectorizeError> {
    Ok(vectorize_bytes(bytes, options)?.to_svg())
}

/// Vectorize encoded image bytes into a [`Document`], for callers that want the
/// geometry or the statistics rather than serialized SVG.
///
/// Requires the `decode` feature, which is on by default.
#[cfg(feature = "decode")]
pub fn vectorize_bytes(
    bytes: &[u8],
    options: &VectorizeOptions,
) -> Result<Document, VectorizeError> {
    let image = image::load_from_memory(bytes)?.to_rgba8();
    Ok(vectorize_image(&image, options))
}

/// Vectorize raw, non-premultiplied RGBA8 pixels.
///
/// The entry point for callers that already have decoded pixels — notably the
/// browser, which decodes far more formats than any bundled codec set and does it
/// without adding to the wasm payload.
pub fn vectorize_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    options: &VectorizeOptions,
) -> Result<Document, VectorizeError> {
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(VectorizeError::Vectorize(format!(
            "expected {expected} bytes for {width}x{height} RGBA, got {}",
            rgba.len()
        )));
    }
    let image = RgbaImage::from_raw(width, height, rgba.to_vec()).ok_or_else(|| {
        VectorizeError::Vectorize("could not build an image from those pixels".to_string())
    })?;
    Ok(vectorize_image(&image, options))
}

/// Vectorize an already-decoded image.
pub fn vectorize_image(image: &RgbaImage, options: &VectorizeOptions) -> Document {
    let resolved = resolve_auto_options(image, options);
    vectorize::vectorize(image, &resolved)
}

#[cfg(all(target_arch = "wasm32", feature = "decode"))]
#[wasm_bindgen]
pub fn png_to_svg_wasm(png_bytes: &[u8], options_json: &str) -> Result<String, JsValue> {
    let options = parse_options(options_json)?;
    png_to_svg(png_bytes, &options).map_err(|err| JsValue::from_str(&err.to_string()))
}

/// Vectorize decoded RGBA pixels and return the SVG.
///
/// Preferred over [`png_to_svg_wasm`] in the browser: the page decodes the file
/// itself, so every format it supports works and no codec is compiled into the
/// wasm.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn vectorize_rgba_wasm(
    width: u32,
    height: u32,
    rgba: &[u8],
    options_json: &str,
) -> Result<String, JsValue> {
    let options = parse_options(options_json)?;
    vectorize_rgba(width, height, rgba, &options)
        .map(|document| document.to_svg())
        .map_err(|err| JsValue::from_str(&err.to_string()))
}

/// Vectorize decoded RGBA pixels and return the SVG plus a summary, as JSON.
///
/// Lets the page show node counts and accuracy next to the result, which is the
/// pair of numbers that actually describes the quality of a trace.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn vectorize_rgba_report_wasm(
    width: u32,
    height: u32,
    rgba: &[u8],
    options_json: &str,
) -> Result<String, JsValue> {
    let options = parse_options(options_json)?;
    let expected = width as usize * height as usize * 4;
    if rgba.len() != expected {
        return Err(JsValue::from_str("pixel buffer has the wrong length"));
    }
    let image = RgbaImage::from_raw(width, height, rgba.to_vec())
        .ok_or_else(|| JsValue::from_str("could not build an image from those pixels"))?;

    let document = vectorize_image(&image, &options);
    let stats = document.stats();
    let report = serde_json::json!({
        "svg": document.to_svg(),
        "shapes": stats.shapes,
        "nodes": stats.nodes,
        "circles": stats.circles,
        "ellipses": stats.ellipses,
        "rects": stats.rects,
        "accuracy": accuracy(&document, &image),
    });
    serde_json::to_string(&report).map_err(|err| JsValue::from_str(&err.to_string()))
}

#[cfg(target_arch = "wasm32")]
fn parse_options(options_json: &str) -> Result<VectorizeOptions, JsValue> {
    if options_json.trim().is_empty() {
        Ok(VectorizeOptions::default())
    } else {
        serde_json::from_str::<VectorizeOptions>(options_json)
            .map_err(|err| JsValue::from_str(&format!("invalid options json: {err}")))
    }
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn default_options_json() -> String {
    serde_json::to_string(&VectorizeOptions::default()).unwrap_or_else(|_| "{}".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::png::PngEncoder, ColorType, ImageEncoder, Rgba};
    use serde_json::json;

    fn encode(image: &RgbaImage) -> Vec<u8> {
        let mut bytes = Vec::new();
        PngEncoder::new(&mut bytes)
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                ColorType::Rgba8.into(),
            )
            .expect("image should encode");
        bytes
    }

    #[test]
    fn produces_a_well_formed_svg() {
        let image = RgbaImage::from_fn(16, 16, |x, y| {
            if (4..12).contains(&x) && (4..12).contains(&y) {
                Rgba([200, 40, 40, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let svg = png_to_svg(&encode(&image), &VectorizeOptions::default())
            .expect("vectorization should succeed");

        assert!(svg.starts_with("<svg"));
        assert!(svg.contains("viewBox=\"0 0 16 16\""));
        assert!(svg.trim_end().ends_with("</svg>"));
    }

    #[test]
    fn options_round_trip_json() {
        let json = json!({
            "colors": 12,
            "detail": 0.75,
            "smoothness": 0.4,
            "tolerance": 2.0,
            "mode": "pixel",
        });

        let options: VectorizeOptions =
            serde_json::from_value(json).expect("options should deserialize");
        assert_eq!(options.colors, 12);
        assert_eq!(options.mode, VectorizeMode::PixelArt);

        let serialized = serde_json::to_string(&options).expect("options should serialize");
        assert!(serialized.contains("\"mode\":\"pixel\""));
    }

    #[test]
    fn default_options_start_in_auto_mode() {
        assert_eq!(VectorizeOptions::default().mode, VectorizeMode::Auto);
    }

    #[test]
    fn auto_mode_infers_logo_for_a_transparent_mark() {
        let image = RgbaImage::from_fn(32, 32, |x, y| {
            let dx = x as i32 - 16;
            let dy = y as i32 - 16;
            let distance_sq = dx * dx + dy * dy;
            if (70..=190).contains(&distance_sq) {
                Rgba([30, 30, 34, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });

        let options = resolve_auto_options(&image, &VectorizeOptions::default());
        assert_eq!(options.mode, VectorizeMode::Logo);
        assert!(options.smoothness > 0.5);
    }

    #[test]
    fn auto_mode_keeps_tiny_hard_edged_art_crisp() {
        let image = RgbaImage::from_fn(12, 12, |x, y| {
            if x == y || x + 1 == y {
                Rgba([20, 20, 20, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });

        let options = resolve_auto_options(&image, &VectorizeOptions::default());
        assert_eq!(options.mode, VectorizeMode::PixelArt);
        assert_eq!(options.smoothness, 0.0);
    }

    #[test]
    fn an_empty_canvas_produces_no_shapes() {
        let image = RgbaImage::from_pixel(8, 8, Rgba([0, 0, 0, 0]));
        let document = vectorize_image(&image, &VectorizeOptions::default());
        assert!(document.shapes.is_empty());
        let svg = document.to_svg();
        assert!(svg.contains("<svg"));
        assert!(!svg.contains("<path"));
    }

    #[test]
    fn output_is_deterministic() {
        let image = RgbaImage::from_fn(24, 24, |x, y| {
            let dx = x as f32 - 12.0;
            let dy = y as f32 - 12.0;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance < 8.0 {
                Rgba([220, 40, 40, 255])
            } else if distance < 10.0 {
                Rgba([40, 90, 220, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let bytes = encode(&image);
        let options = VectorizeOptions::default();

        let first = png_to_svg(&bytes, &options).expect("should vectorize");
        for _ in 0..8 {
            assert_eq!(first, png_to_svg(&bytes, &options).expect("should vectorize"));
        }
    }
}
