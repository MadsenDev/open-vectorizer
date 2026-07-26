//! Tool-agnostic scoring: compare a rendered SVG against the source raster, and
//! count the geometry an SVG actually uses.
//!
//! Both metrics are deliberately independent of Open Vectorizer's internals, so
//! the comparison does not quietly favour our own model of the image.

use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("alpha") => compare(&args[2], &args[3], false),
        Some("rgba") => compare(&args[2], &args[3], true),
        Some("nodes") => println!("{}", count_nodes(&fs::read_to_string(&args[2]).unwrap())),
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

/// Count the on-curve nodes an SVG uses.
///
/// Path data is parsed properly rather than counting command letters: SVG allows
/// implicit repetition, so `C` followed by twelve numbers is two cubics under one
/// letter, and letter-counting would report half the real geometry. Primitive
/// elements count as one node each, which is the point of emitting them.
fn count_nodes(svg: &str) -> usize {
    let mut total = 0usize;

    for element in ["<circle", "<ellipse", "<rect", "<line"] {
        total += svg.matches(element).count();
    }
    for points in attribute_values(svg, "points") {
        total += scan_numbers(&points).len() / 2;
    }
    for data in attribute_values(svg, "d") {
        total += count_path_segments(&data);
    }

    total
}

/// Every value of `name="..."` in the document.
fn attribute_values(svg: &str, name: &str) -> Vec<String> {
    let needle = format!("{name}=\"");
    let mut values = Vec::new();
    let mut rest = svg;
    while let Some(start) = rest.find(&needle) {
        // Guard against matching the tail of a longer attribute name.
        let preceding = rest[..start].chars().next_back();
        rest = &rest[start + needle.len()..];
        if matches!(preceding, Some(character) if character.is_alphanumeric() || character == '-') {
            continue;
        }
        if let Some(end) = rest.find('"') {
            values.push(rest[..end].to_string());
            rest = &rest[end + 1..];
        }
    }
    values
}

fn count_path_segments(data: &str) -> usize {
    let mut segments = 0usize;
    let mut index = 0usize;
    let bytes = data.as_bytes();
    let mut command = b' ';

    while index < bytes.len() {
        let byte = bytes[index];
        if byte.is_ascii_alphabetic() {
            command = byte;
            index += 1;
            if command == b'z' || command == b'Z' {
                // Closepath produces no new node.
                continue;
            }
            // A moveto's first coordinate pair positions the pen; only the
            // repeats that follow are line segments.
            let arity = command_arity(command);
            if let Some(consumed) = take_numbers(&data[index..], arity) {
                index += consumed;
                if command != b'M' && command != b'm' {
                    segments += 1;
                }
            }
            continue;
        }

        if byte.is_ascii_whitespace() || byte == b',' {
            index += 1;
            continue;
        }

        // A number with no preceding letter repeats the current command.
        let arity = command_arity(command);
        match take_numbers(&data[index..], arity) {
            Some(consumed) if consumed > 0 => {
                index += consumed;
                segments += 1;
            }
            _ => index += 1,
        }
    }

    segments
}

fn command_arity(command: u8) -> usize {
    match command.to_ascii_uppercase() {
        b'M' | b'L' | b'T' => 2,
        b'H' | b'V' => 1,
        b'C' => 6,
        b'S' | b'Q' => 4,
        b'A' => 7,
        _ => 0,
    }
}

/// Consume exactly `count` numbers, returning how many bytes that took.
fn take_numbers(text: &str, count: usize) -> Option<usize> {
    if count == 0 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut index = 0usize;

    for _ in 0..count {
        while index < bytes.len() && (bytes[index].is_ascii_whitespace() || bytes[index] == b',') {
            index += 1;
        }
        let start = index;
        if index < bytes.len() && (bytes[index] == b'-' || bytes[index] == b'+') {
            index += 1;
        }
        let mut seen_digit = false;
        while index < bytes.len() && bytes[index].is_ascii_digit() {
            index += 1;
            seen_digit = true;
        }
        if index < bytes.len() && bytes[index] == b'.' {
            index += 1;
            while index < bytes.len() && bytes[index].is_ascii_digit() {
                index += 1;
                seen_digit = true;
            }
        }
        if !seen_digit {
            return None;
        }
        if index < bytes.len() && (bytes[index] == b'e' || bytes[index] == b'E') {
            let mark = index;
            index += 1;
            if index < bytes.len() && (bytes[index] == b'-' || bytes[index] == b'+') {
                index += 1;
            }
            if index < bytes.len() && bytes[index].is_ascii_digit() {
                while index < bytes.len() && bytes[index].is_ascii_digit() {
                    index += 1;
                }
            } else {
                index = mark;
            }
        }
        if index == start {
            return None;
        }
    }

    Some(index)
}

fn scan_numbers(text: &str) -> Vec<f64> {
    let mut numbers = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_ascii_digit()
            || character == '.'
            || character == '-'
            || character == '+'
            || character == 'e'
            || character == 'E'
        {
            if (character == '-' || character == '+')
                && !current.is_empty()
                && !current.ends_with('e')
                && !current.ends_with('E')
            {
                if let Ok(value) = current.parse() {
                    numbers.push(value);
                }
                current.clear();
            }
            current.push(character);
        } else {
            if let Ok(value) = current.parse() {
                numbers.push(value);
            }
            current.clear();
        }
    }
    if let Ok(value) = current.parse() {
        numbers.push(value);
    }
    numbers
}
