use std::fs;
use std::path::PathBuf;
use std::process;

use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use png2svg_core::{png_to_svg, VectorizeMode, VectorizeOptions};

/// Minimal CLI wrapper around the png2svg core engine.
#[derive(Parser, Debug)]
#[command(
    name = "png2svg",
    about = "Convert PNG assets into SVGs (stub engine)",
    long_about = "Convert PNG assets into SVGs with palette reduction and basic grouping."
)]
struct Cli {
    /// Path to the input PNG file.
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
    /// Tolerance for path simplification and grouping (higher = looser).
    #[arg(
        short = 't',
        long,
        default_value_t = 1.5,
        value_parser = parse_tolerance,
        help = "Controls how aggressively nearby segments are merged (0.1-10.0)."
    )]
    tolerance: f32,
    /// Rendering mode hint.
    #[arg(
        long,
        default_value = "logo",
        value_parser = parse_mode,
        value_name = "logo|poster|pixel",
        help = "Preset tuned for logo, poster, or pixel-art inputs."
    )]
    mode: VectorizeMode,
    /// Print debug info about the parsed options.
    #[arg(long, action = ArgAction::SetTrue)]
    debug: bool,
}

fn parse_mode(mode: &str) -> Result<VectorizeMode, String> {
    match mode.to_lowercase().as_str() {
        "logo" => Ok(VectorizeMode::Logo),
        "poster" => Ok(VectorizeMode::Poster),
        "pixel" | "pixel-art" | "pixelart" => Ok(VectorizeMode::PixelArt),
        _ => Err("mode must be one of: logo, poster, pixel".into()),
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

    let svg = png_to_svg(&png_bytes, &options)?;

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
