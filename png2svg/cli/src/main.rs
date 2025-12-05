use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use png2svg_core::{png_to_svg, VectorizeMode, VectorizeOptions};

/// Minimal CLI wrapper around the png2svg core engine.
#[derive(Parser, Debug)]
#[command(name = "png2svg", about = "Convert PNG assets into SVGs (stub engine)")]
struct Cli {
    /// Path to the input PNG file.
    input: PathBuf,
    /// Optional path to write the SVG output. Defaults to stdout.
    #[arg(short, long)]
    output: Option<PathBuf>,
    /// Number of colors to quantize the image to.
    #[arg(short = 'c', long, default_value_t = 8)]
    colors: u8,
    /// Desired detail level (0.0 - 1.0)
    #[arg(short = 'd', long, default_value_t = 0.5)]
    detail: f32,
    /// Smoothness factor for curves (0.0 - 1.0)
    #[arg(short = 's', long, default_value_t = 0.5)]
    smoothness: f32,
    /// Tolerance for path simplification and grouping (higher = looser).
    #[arg(short = 't', long, default_value_t = 1.5)]
    tolerance: f32,
    /// Rendering mode hint.
    #[arg(long, default_value = "logo", value_parser = parse_mode)]
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

fn main() -> Result<()> {
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
