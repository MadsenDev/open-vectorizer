//! Tool-agnostic scoring: compare a rendered SVG against the source raster, and
//! count the geometry an SVG actually uses.
//!
//! Both metrics are deliberately independent of Open Vectorizer's internals, so
//! the comparison does not quietly favour our own model of the image.

use shootout::nodes;
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("alpha") => compare(&args[2], &args[3], false),
        Some("rgba") => compare(&args[2], &args[3], true),
        Some("nodes") => println!("{}", nodes::count(&fs::read_to_string(&args[2]).unwrap())),
        _ => {
            eprintln!("usage: score alpha|rgba <source.png> <rendered.png> | score nodes <f.svg>");
            std::process::exit(2);
        }
    }
}

fn compare(source_path: &str, rendered_path: &str, colour: bool) {
    let source = image::open(source_path).unwrap().to_rgba8();
    let rendered = image::open(rendered_path).unwrap().to_rgba8();

    if source.dimensions() != rendered.dimensions() {
        eprintln!(
            "dimension mismatch: {:?} vs {:?}",
            source.dimensions(),
            rendered.dimensions()
        );
        std::process::exit(3);
    }

    let mut sum = 0.0f64;
    let mut worst = 0.0f64;
    let mut source_area = 0.0f64;
    let mut rendered_area = 0.0f64;
    let channels = if colour { 4 } else { 1 };

    for (a, b) in source.pixels().zip(rendered.pixels()) {
        // Premultiplied, so a colour difference in a nearly transparent pixel
        // does not count for more than the pixel is worth.
        let pa = premultiplied(a.0);
        let pb = premultiplied(b.0);

        let mut pixel_error = 0.0;
        if colour {
            for index in 0..4 {
                pixel_error += (pa[index] - pb[index]).abs();
            }
            pixel_error /= 4.0;
        } else {
            pixel_error = (pa[3] - pb[3]).abs();
        }
        sum += pixel_error;
        worst = worst.max(pixel_error);

        source_area += pa[3] as f64;
        rendered_area += pb[3] as f64;
    }

    let pixels = (source.width() * source.height()) as f64;
    let _ = channels;
    println!(
        "{:.6} {:.4} {:.1}",
        1.0 - sum / pixels,
        worst,
        rendered_area - source_area
    );
}

fn premultiplied(rgba: [u8; 4]) -> [f64; 4] {
    let alpha = rgba[3] as f64 / 255.0;
    [
        rgba[0] as f64 / 255.0 * alpha,
        rgba[1] as f64 / 255.0 * alpha,
        rgba[2] as f64 / 255.0 * alpha,
        alpha,
    ]
}
