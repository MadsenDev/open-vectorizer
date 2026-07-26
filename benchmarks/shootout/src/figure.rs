//! Build the comparison figure used at the top of the README.
//!
//! The figure exists because the difference between these engines is invisible
//! at a glance: three renderings of the same circle look like three circles.
//! What differs is the geometry underneath, so the figure draws that — every
//! on-curve node each engine emitted, over its own outline.
//!
//! Node positions come from `shootout::nodes`, the same parser that produces the
//! numbers in the tables, so a reader can count the dots in the picture and get
//! the number printed under it.
//!
//! Usage: `figure <cases-dir> <out-dir>`, after `gen` and `run.sh` have
//! populated the cases directory.

use shootout::nodes::{self, Element};
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

/// Panel geometry, in output pixels.
const PANEL: f64 = 190.0;
const GAP: f64 = 14.0;
const CAPTION: f64 = 26.0;
const MARGIN: f64 = 20.0;
const HEADER: f64 = 52.0;

const INK: &str = "#1d2430";
const MUTED: &str = "#6b7789";
const FILL: &str = "#dde5ef";
const OUTLINE: &str = "#48586e";
const NODE: &str = "#d92d20";
const OURS_CARD: &str = "#f2f7fd";
const CARD: &str = "#fafbfc";
const RULE: &str = "#e3e7ec";

/// The cases the figure shows, in order.
///
/// All four are single-colour, which is what lets every column be filled:
/// potrace is a 1-bit tracer and has no colour output to show. They were picked
/// to cover the three things the engine claims — a recovered primitive, an
/// exact corner, and sub-pixel placement on a small icon — plus a shape with
/// nothing to exploit.
const CASES: &[(&str, &str)] = &[
    ("circle", "recovered as a primitive"),
    ("rotated-square", "corners reconstructed"),
    ("star5", "ten sharp points"),
    ("blob", "no primitive to exploit"),
];

/// The columns, as (heading, sub-heading, filename suffix).
const COLUMNS: &[(&str, &str, &str)] = &[
    ("Input", "anti-aliased raster", ""),
    ("Open Vectorizer", "anti-aliased input", "ours"),
    ("VTracer 0.6.5", "anti-aliased input", "vtracer"),
    ("potrace 1.16", "thresholded bilevel", "potrace"),
];

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: figure <cases-dir> <out-dir>");
        std::process::exit(2);
    }
    let cases = Path::new(&args[1]);
    let out = Path::new(&args[2]);
    fs::create_dir_all(out).unwrap();

    let manifest = fs::read_to_string(cases.join("manifest.tsv")).unwrap();
    let sizes: Vec<(String, f64, f64)> = manifest
        .lines()
        .filter_map(|line| {
            let mut fields = line.split('\t');
            Some((
                fields.next()?.to_string(),
                fields.next()?.parse().ok()?,
                fields.next()?.parse().ok()?,
            ))
        })
        .collect();

    let svg = compose(cases, &sizes);
    let path = out.join("comparison.svg");
    fs::write(&path, svg).unwrap();
    println!("wrote {}", path.display());
}

fn compose(cases: &Path, sizes: &[(String, f64, f64)]) -> String {
    let columns = COLUMNS.len() as f64;
    let width = MARGIN * 2.0 + columns * PANEL + (columns - 1.0) * GAP;
    let row_height = PANEL + CAPTION + GAP;
    let height = MARGIN + HEADER + CASES.len() as f64 * row_height + MARGIN;

    let mut svg = String::new();
    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="{width}" height="{height}" viewBox="0 0 {width} {height}">
<style>
  text {{ font-family: "DejaVu Sans", "Helvetica Neue", Arial, sans-serif; }}
  .h {{ font-size: 13px; font-weight: bold; fill: {INK}; }}
  .sub {{ font-size: 10.5px; fill: {MUTED}; }}
  .cap {{ font-size: 11.5px; fill: {INK}; }}
  .capsub {{ font-size: 10.5px; fill: {MUTED}; }}
</style>
<rect width="{width}" height="{height}" fill="#ffffff"/>
"##
    );

    for (index, (heading, sub, _)) in COLUMNS.iter().enumerate() {
        let x = MARGIN + index as f64 * (PANEL + GAP) + PANEL / 2.0;
        let _ = writeln!(
            svg,
            r#"<text class="h" x="{x:.1}" y="{y:.1}" text-anchor="middle">{heading}</text>
<text class="sub" x="{x:.1}" y="{sy:.1}" text-anchor="middle">{sub}</text>"#,
            y = MARGIN + 14.0,
            sy = MARGIN + 29.0,
        );
    }
    let _ = writeln!(
        svg,
        r#"<line x1="{MARGIN}" y1="{y:.1}" x2="{x2:.1}" y2="{y:.1}" stroke="{RULE}"/>"#,
        y = MARGIN + HEADER - 14.0,
        x2 = width - MARGIN,
    );

    for (row, (case, note)) in CASES.iter().enumerate() {
        let (_, case_width, case_height) = sizes
            .iter()
            .find(|(name, _, _)| name == case)
            .unwrap_or_else(|| panic!("case {case} is not in the manifest"));
        let top = MARGIN + HEADER + row as f64 * row_height;

        for (column, (_, _, suffix)) in COLUMNS.iter().enumerate() {
            let left = MARGIN + column as f64 * (PANEL + GAP);
            let card = if column == 1 { OURS_CARD } else { CARD };
            let _ = writeln!(
                svg,
                r#"<rect x="{left:.1}" y="{top:.1}" width="{PANEL}" height="{PANEL}" rx="4" fill="{card}" stroke="{RULE}"/>"#
            );

            let caption_y = top + PANEL + 15.0;
            let centre = left + PANEL / 2.0;

            if suffix.is_empty() {
                let png = cases.join(format!("{case}.white.png"));
                let _ = writeln!(
                    svg,
                    r#"<image x="{x:.2}" y="{y:.2}" width="{w:.2}" height="{h:.2}" xlink:href="data:image/png;base64,{data}"/>"#,
                    x = left + (PANEL - fitted(*case_width, *case_width, *case_height)) / 2.0,
                    y = top + (PANEL - fitted(*case_height, *case_width, *case_height)) / 2.0,
                    w = fitted(*case_width, *case_width, *case_height),
                    h = fitted(*case_height, *case_width, *case_height),
                    data = base64(&fs::read(&png).unwrap()),
                );
                let _ = writeln!(
                    svg,
                    r#"<text class="cap" x="{centre:.1}" y="{caption_y:.1}" text-anchor="middle">{case}</text>
<text class="capsub" x="{centre:.1}" y="{y2:.1}" text-anchor="middle">{w:.0}×{h:.0} · {note}</text>"#,
                    y2 = caption_y + 14.0,
                    w = case_width,
                    h = case_height,
                );
                continue;
            }

            let path = cases.join(format!("{case}.{suffix}.svg"));
            let source = fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("missing {} — run gen and run.sh first", path.display()));
            let parsed = nodes::elements(&source);
            let marks = foreground(&parsed, *case_width, *case_height);
            let counted: usize = marks.iter().map(|e| e.points.len()).sum();

            svg.push_str(&panel(&marks, left, top, *case_width, *case_height));
            let _ = writeln!(
                svg,
                r#"<text class="cap" x="{centre:.1}" y="{caption_y:.1}" text-anchor="middle">{counted} node{plural}</text>"#,
                plural = if counted == 1 { "" } else { "s" },
            );
        }
    }

    svg.push_str("</svg>\n");
    svg
}

/// The size a case's dimension takes inside a square panel, preserving aspect.
fn fitted(dimension: f64, width: f64, height: f64) -> f64 {
    dimension * (PANEL / width.max(height))
}

/// Drop the background rectangle an engine emits when handed an opaque image.
///
/// VTracer is built for opaque input and paints a full-canvas white rectangle
/// under everything; on the same input we emit a white shape with the artwork
/// punched out of it. Neither is part of the mark, and leaving them in would
/// scatter markers around the border of half the panels. The rule is applied to
/// every engine identically: a near-white shape spanning the whole canvas is
/// background. The tables below the figure count the full file, backgrounds
/// included.
fn foreground(elements: &[Element], width: f64, height: f64) -> Vec<&Element> {
    elements
        .iter()
        .filter(|element| {
            let Some(fill) = element.fill.as_deref().map(str::trim) else {
                return true;
            };
            if !near_white(fill) {
                return true;
            }
            let xs = element.points.iter().map(|p| p.0);
            let ys = element.points.iter().map(|p| p.1);
            let span_x = xs.clone().fold(f64::MAX, f64::min)..xs.fold(f64::MIN, f64::max);
            let span_y = ys.clone().fold(f64::MAX, f64::min)..ys.fold(f64::MIN, f64::max);
            let covers = (span_x.end - span_x.start) >= width * 0.98
                && (span_y.end - span_y.start) >= height * 0.98;
            !covers
        })
        .collect()
}

fn near_white(fill: &str) -> bool {
    let Some(hex) = fill.strip_prefix('#') else {
        return fill.eq_ignore_ascii_case("white");
    };
    let expanded: String = if hex.len() == 3 {
        hex.chars().flat_map(|c| [c, c]).collect()
    } else {
        hex.to_string()
    };
    if expanded.len() < 6 {
        return false;
    }
    (0..3).all(|channel| {
        u8::from_str_radix(&expanded[channel * 2..channel * 2 + 2], 16).unwrap_or(0) >= 240
    })
}

/// One engine panel: the shape, its outline, and a marker on every node.
fn panel(marks: &[&Element], left: f64, top: f64, width: f64, height: f64) -> String {
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        r#"<svg x="{left:.1}" y="{top:.1}" width="{PANEL}" height="{PANEL}" viewBox="0 0 {width} {height}">"#
    );

    // The shape, then its outline, then the markers, so nothing is buried.
    for element in marks {
        svg.push_str(&wrapped(element, &format!(r#"fill="{FILL}""#)));
    }
    for element in marks {
        svg.push_str(&wrapped(
            element,
            &format!(
                r#"fill="none" stroke="{OUTLINE}" stroke-width="1.1" vector-effect="non-scaling-stroke""#
            ),
        ));
    }

    // Markers are sized in output pixels, so they stay legible whatever the
    // case's resolution or the engine's internal coordinate scale — potrace
    // works in tenths of a unit and would otherwise draw dots ten times too big.
    let panel_scale = PANEL / width.max(height);
    for element in marks {
        let radius = 3.4 / (panel_scale * transform_scale(&element.transforms));
        let stroke = 1.1 / (panel_scale * transform_scale(&element.transforms));
        let mut dots = String::new();
        for (x, y) in &element.points {
            let _ = write!(
                dots,
                r##"<circle cx="{x}" cy="{y}" r="{radius}" fill="{NODE}" stroke="#ffffff" stroke-width="{stroke}"/>"##
            );
        }
        svg.push_str(&in_transforms(&element.transforms, &dots));
    }

    svg.push_str("</svg>\n");
    svg
}

/// Re-emit an element with our own paint, inside its original transforms.
fn wrapped(element: &Element, paint: &str) -> String {
    let mut tag = element.markup.clone();
    for attribute in [
        "fill",
        "stroke",
        "stroke-width",
        "style",
        "opacity",
        "fill-opacity",
        "transform",
        "vector-effect",
    ] {
        tag = strip_attribute(&tag, attribute);
    }
    let tag = tag.replacen(
        &format!("<{}", element.tag),
        &format!("<{} {paint}", element.tag),
        1,
    );
    in_transforms(&element.transforms, &tag)
}

fn in_transforms(transforms: &[String], inner: &str) -> String {
    let mut open = String::new();
    let mut close = String::new();
    for transform in transforms {
        let _ = write!(open, r#"<g transform="{transform}">"#);
        close.push_str("</g>");
    }
    format!("{open}{inner}{close}\n")
}

fn strip_attribute(tag: &str, name: &str) -> String {
    let needle = format!("{name}=\"");
    let mut result = String::new();
    let mut rest = tag;
    while let Some(start) = rest.find(&needle) {
        let preceding = rest[..start].chars().next_back();
        if matches!(preceding, Some(c) if c.is_alphanumeric() || c == '-' || c == ':') {
            let advance = start + needle.len();
            result.push_str(&rest[..advance]);
            rest = &rest[advance..];
            continue;
        }
        result.push_str(&rest[..start]);
        let after = &rest[start + needle.len()..];
        rest = after.find('"').map_or("", |end| &after[end + 1..]);
    }
    result.push_str(rest);
    result
}

/// How much a transform chain scales lengths, as a single factor.
///
/// Only the magnitude matters here, and every transform these engines emit is a
/// translate, a scale, or a matrix, so the area factor is enough.
fn transform_scale(transforms: &[String]) -> f64 {
    let mut scale = 1.0;
    for transform in transforms {
        let mut rest = transform.as_str();
        while let Some(open) = rest.find('(') {
            let name = rest[..open]
                .rsplit(|c: char| c == ')' || c.is_whitespace() || c == ',')
                .next()
                .unwrap_or("")
                .trim()
                .to_string();
            let Some(close) = rest[open..].find(')') else { break };
            let numbers = nodes::scan_numbers(&rest[open + 1..open + close]);
            rest = &rest[open + close + 1..];
            match name.as_str() {
                "scale" => {
                    let x = numbers.first().copied().unwrap_or(1.0);
                    let y = numbers.get(1).copied().unwrap_or(x);
                    scale *= (x * y).abs().sqrt();
                }
                "matrix" if numbers.len() >= 4 => {
                    let determinant = numbers[0] * numbers[3] - numbers[1] * numbers[2];
                    scale *= determinant.abs().sqrt();
                }
                _ => {}
            }
        }
    }
    if scale > 0.0 {
        scale
    } else {
        1.0
    }
}

/// Standard base64, so the input raster can be embedded in the figure and the
/// figure stays a single file.
fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] =
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let mut buffer = [0u8; 3];
        buffer[..chunk.len()].copy_from_slice(chunk);
        let packed = u32::from(buffer[0]) << 16 | u32::from(buffer[1]) << 8 | u32::from(buffer[2]);
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(ALPHABET[(packed >> (18 - index * 6) & 0x3f) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}
