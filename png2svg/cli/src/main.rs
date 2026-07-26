use std::fs;
use std::path::PathBuf;
use std::process;

use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use png2svg_core::{accuracy, vectorize_bytes, VectorizeMode, VectorizeOptions};

/// CLI wrapper around the png2svg core engine.
#[derive(Parser, Debug)]
#[command(
    name = "png2svg",
    about = "Convert raster logos and icons into clean SVG",
    long_about = "Convert raster images into clean, editable SVG.\n\n\
                  Reads anti-aliased edges as sub-pixel coverage rather than as \
                  colors, recovers corners and primitives, and picks the simplest \
                  geometry that still reproduces the input. Circles, ellipses and \
                  rectangles are emitted as real SVG elements. Runs entirely \
                  locally and is deterministic."
)]
struct Cli {
    /// Path to the input image (PNG, JPEG, WebP, GIF, BMP, ...).
    input: PathBuf,
    /// Optional path to write the SVG output. Defaults to stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Number of colors to quantize the image to.
    #[arg(
        short = 'c',
        long,
        default_value_t = 8,
        value_parser = parse_colors,
        help = "Number of colors to quantize the image to (2-64)."
    )]
    colors: u8,
    /// Desired detail level (0.0 - 1.0)
    #[arg(
        short = 'd',
        long,
        default_value_t = 0.5,
        value_parser = parse_detail,
        help = "Higher detail preserves more small features; valid range is 0.0-1.0."
    )]
    detail: f32,
    /// Smoothness factor for curves (0.0 - 1.0)
    #[arg(
        short = 's',
        long,
        default_value_t = 0.5,
        value_parser = parse_smoothness,
        help = "Higher smoothness softens edges; valid range is 0.0-1.0."
    )]
    smoothness: f32,
    /// Geometric error ceiling (higher = looser, fewer nodes).
    #[arg(
        short = 't',
        long,
        default_value_t = 1.5,
        value_parser = parse_tolerance,
        help = "Geometric error ceiling, 0.1-10.0. A quarter of this is the \
                budget in pixels, so the default 1.5 allows about 0.38px."
    )]
    tolerance: f32,
    /// Report shape and node counts, and how well the SVG reproduces the input.
    #[arg(long, action = ArgAction::SetTrue)]
    stats: bool,
    /// Rendering mode hint.
    #[arg(
        long,
        default_value = "auto",
        value_parser = parse_mode,
        value_name = "auto|logo|poster|pixel",
        help = "Preset tuned for automatic detection, logo, poster, or pixel-art inputs."
    )]
    mode: VectorizeMode,
    /// Print debug info about the parsed options.
    #[arg(long, action = ArgAction::SetTrue)]
    debug: bool,
}

fn parse_mode(mode: &str) -> Result<VectorizeMode, String> {
    match mode.to_lowercase().as_str() {
        "auto" => Ok(VectorizeMode::Auto),
        "logo" => Ok(VectorizeMode::Logo),
        "poster" => Ok(VectorizeMode::Poster),
        "pixel" | "pixel-art" | "pixelart" => Ok(VectorizeMode::PixelArt),
        _ => Err("mode must be one of: auto, logo, poster, pixel".into()),
    }
}

fn parse_colors(value: &str) -> Result<u8, String> {
    parse_u8_range(value, "colors", 2, 64)
}

fn parse_detail(value: &str) -> Result<f32, String> {
    parse_f32_range(value, "detail", 0.0, 1.0)
}

fn parse_smoothness(value: &str) -> Result<f32, String> {
    parse_f32_range(value, "smoothness", 0.0, 1.0)
}

fn parse_tolerance(value: &str) -> Result<f32, String> {
    parse_f32_range(value, "tolerance", 0.1, 10.0)
}

fn parse_f32_range(value: &str, name: &str, min: f32, max: f32) -> Result<f32, String> {
    let parsed: f32 = value
        .parse()
        .map_err(|_| format!("{name} must be a number between {min} and {max}"))?;
    if (min..=max).contains(&parsed) {
        Ok(parsed)
    } else {
        Err(format!("{name} must be between {min} and {max}"))
    }
}

fn parse_u8_range(value: &str, name: &str, min: u8, max: u8) -> Result<u8, String> {
    let parsed: u8 = value
        .parse()
        .map_err(|_| format!("{name} must be a whole number between {min} and {max}"))?;
    if (min..=max).contains(&parsed) {
        Ok(parsed)
    } else {
        Err(format!("{name} must be between {min} and {max}"))
    }
}

fn main() {
    if let Err(err) = run() {
        eprintln!("[open-vectorizer] error: {err}");
        for cause in err.chain().skip(1) {
            eprintln!("  caused by: {cause}");
        }
        process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();

    let png_bytes = fs::read(&cli.input)
        .with_context(|| format!("failed to read input file: {}", cli.input.display()))?;

    let options = VectorizeOptions {
        colors: cli.colors,
        detail: cli.detail,
        smoothness: cli.smoothness,
        tolerance: cli.tolerance,
        mode: cli.mode,
    };

    if cli.debug {
        eprintln!("[open-vectorizer] options: {:?}", options);
    }

    let document = vectorize_bytes(&png_bytes, &options)?;

    if cli.stats {
        let stats = document.stats();
        // Decode once more to score the result against the source. Reporting
        // accuracy alongside the node count is the honest pair of numbers: a
        // vectorizer can always be more accurate by emitting more geometry.
        let score = image::load_from_memory(&png_bytes)
            .map(|image| accuracy(&document, &image.to_rgba8()))
            .unwrap_or(f64::NAN);

        eprintln!(
            "[open-vectorizer] {} shapes, {} nodes ({} circles, {} ellipses, {} rects), accuracy {:.5}",
            stats.shapes, stats.nodes, stats.circles, stats.ellipses, stats.rects, score
        );
    }

    let svg = document.to_svg();

    match cli.output {
        Some(path) => {
            fs::write(&path, svg).with_context(|| format!("failed to write {}", path.display()))?;
        }
        None => {
            println!("{}", svg);
        }
    }

    Ok(())
}
