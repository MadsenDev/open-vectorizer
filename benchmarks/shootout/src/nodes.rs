//! Find the on-curve nodes an SVG actually uses, wherever it came from.
//!
//! This is deliberately independent of Open Vectorizer's internals: it reads the
//! finished file, the same way any other consumer of the SVG would, so the
//! comparison cannot quietly favour our own model of the image.
//!
//! Two things make this more than counting command letters:
//!
//! * SVG allows implicit repetition. A `C` followed by twelve numbers is two
//!   cubics under one letter, so letter-counting reports half the geometry.
//! * Generators disagree about how to close a shape. potrace ends its last
//!   curve on the starting point and then writes `z`; we write `Z` and let it
//!   draw the closing edge. Counting drawing segments therefore charges potrace
//!   for a node it does not have, and lets us off one we do — a quadrilateral
//!   written `M a L b L c L d Z` has four corners, not three.
//!
//! So this collects the actual on-curve points of each subpath and drops a
//! final point that merely repeats the subpath's start. Both conventions then
//! land on the same number, which is the only way the comparison means
//! anything.

/// Geometry from one element, in that element's own coordinate system.
pub struct Element {
    /// The transforms from the root down to this element, outermost first, in
    /// the order they must be applied. Kept as source text so a consumer can
    /// re-emit it verbatim and let the renderer do the arithmetic.
    pub transforms: Vec<String>,
    /// On-curve nodes, one entry per node.
    pub points: Vec<(f64, f64)>,
    /// The element's own `fill`, when it or an ancestor `<g>` set one.
    pub fill: Option<String>,
    /// The element's tag name, e.g. `path` or `circle`.
    pub tag: String,
    /// The element's markup, so a consumer can re-render the shape itself.
    pub markup: String,
}

/// Total on-curve nodes in the document.
///
/// `<circle>`, `<ellipse>`, `<rect>` and `<line>` count as one node each, which
/// is the point of emitting them: the shape is named rather than approximated.
pub fn count(svg: &str) -> usize {
    elements(svg).iter().map(|e| e.points.len()).sum()
}

/// Every geometry element in the document, in document order.
///
/// The parser handles the structure these three generators actually emit —
/// nested `<g>` with `transform` and `fill`, and geometry elements with
/// attributes. It is not a general SVG implementation and does not try to be:
/// `<use>`, `<defs>`, clip paths and CSS would all need real work, and none of
/// the engines under test produce them.
pub fn elements(svg: &str) -> Vec<Element> {
    let mut found = Vec::new();
    let mut group_transforms: Vec<Option<String>> = Vec::new();
    let mut group_fills: Vec<Option<String>> = Vec::new();
    let mut rest = svg;

    while let Some(open) = rest.find('<') {
        rest = &rest[open..];

        // Comments, the XML declaration and the doctype carry no geometry, and
        // their contents can contain '>' — skip to their real end.
        if let Some(tail) = rest.strip_prefix("<!--") {
            rest = tail.find("-->").map_or("", |end| &tail[end + 3..]);
            continue;
        }
        if rest.starts_with("<?") || rest.starts_with("<!") {
            rest = rest.find('>').map_or("", |end| &rest[end + 1..]);
            continue;
        }

        let Some(close) = rest.find('>') else { break };
        let tag_text = &rest[..=close];
        let inner = &tag_text[1..tag_text.len() - 1];
        rest = &rest[close + 1..];

        if let Some(name) = inner.strip_prefix('/') {
            if name.trim() == "g" {
                group_transforms.pop();
                group_fills.pop();
            }
            continue;
        }

        let name: String = inner
            .chars()
            .take_while(|c| !c.is_whitespace() && *c != '/')
            .collect();
        let self_closing = inner.trim_end().ends_with('/');

        if name == "g" {
            // A self-closing group encloses nothing, so it never opens a scope.
            if !self_closing {
                group_transforms.push(attribute(tag_text, "transform"));
                group_fills.push(
                    attribute(tag_text, "fill").or_else(|| group_fills.last().cloned().flatten()),
                );
            }
            continue;
        }

        let points = match name.as_str() {
            // A named primitive is one node by definition, and its position is
            // the only thing a marker needs.
            "circle" | "ellipse" => vec![(
                attribute_number(tag_text, "cx"),
                attribute_number(tag_text, "cy"),
            )],
            "rect" => vec![(
                attribute_number(tag_text, "x"),
                attribute_number(tag_text, "y"),
            )],
            "line" => vec![(
                attribute_number(tag_text, "x1"),
                attribute_number(tag_text, "y1"),
            )],
            "polygon" | "polyline" => attribute(tag_text, "points")
                .map(|value| {
                    let numbers = scan_numbers(&value);
                    numbers.chunks_exact(2).map(|pair| (pair[0], pair[1])).collect()
                })
                .unwrap_or_default(),
            "path" => attribute(tag_text, "d")
                .map(|data| path_nodes(&data))
                .unwrap_or_default(),
            _ => continue,
        };

        if points.is_empty() {
            continue;
        }

        let mut transforms: Vec<String> = group_transforms.iter().flatten().cloned().collect();
        if let Some(own) = attribute(tag_text, "transform") {
            transforms.push(own);
        }

        found.push(Element {
            transforms,
            points,
            fill: attribute(tag_text, "fill").or_else(|| group_fills.last().cloned().flatten()),
            tag: name,
            markup: tag_text.to_string(),
        });
    }

    found
}

/// The on-curve nodes of a path, subpath by subpath.
fn path_nodes(data: &str) -> Vec<(f64, f64)> {
    let mut nodes = Vec::new();
    let mut subpath: Vec<(f64, f64)> = Vec::new();
    let mut current = (0.0f64, 0.0f64);
    let mut start = (0.0f64, 0.0f64);
    let mut command = b' ';

    let bytes = data.as_bytes();
    let mut index = 0usize;

    // Emit the subpath collected so far, dropping a final point that only
    // repeats the start: with the closing edge counted once either way, the two
    // ways of writing a closed shape agree.
    let flush = |subpath: &mut Vec<(f64, f64)>, nodes: &mut Vec<(f64, f64)>| {
        if subpath.len() > 1 {
            let first = subpath[0];
            let last = subpath[subpath.len() - 1];
            if (first.0 - last.0).abs() < 1e-6 && (first.1 - last.1).abs() < 1e-6 {
                subpath.pop();
            }
        }
        nodes.append(subpath);
    };

    while index < bytes.len() {
        let byte = bytes[index];

        if byte.is_ascii_whitespace() || byte == b',' {
            index += 1;
            continue;
        }

        if byte.is_ascii_alphabetic() {
            command = byte;
            index += 1;
            if command == b'z' || command == b'Z' {
                // Closepath draws the closing edge but introduces no new point.
                flush(&mut subpath, &mut nodes);
                current = start;
                continue;
            }
            if command_arity(command) == 0 {
                continue;
            }
        }

        let arity = command_arity(command);
        if arity == 0 {
            index += 1;
            continue;
        }
        let Some((numbers, consumed)) = take_numbers(&data[index..], arity) else {
            index += 1;
            continue;
        };
        index += consumed;

        let relative = command.is_ascii_lowercase();
        let endpoint = match command.to_ascii_uppercase() {
            b'H' => (numbers[0], if relative { 0.0 } else { current.1 }),
            b'V' => (if relative { 0.0 } else { current.0 }, numbers[0]),
            // For every other command the endpoint is the trailing pair, whether
            // that follows control points, a quadratic, or an arc's flags.
            _ => (numbers[arity - 2], numbers[arity - 1]),
        };
        let endpoint = if relative {
            match command {
                b'h' => (current.0 + endpoint.0, current.1),
                b'v' => (current.0, current.1 + endpoint.1),
                _ => (current.0 + endpoint.0, current.1 + endpoint.1),
            }
        } else {
            endpoint
        };

        if command == b'M' || command == b'm' {
            flush(&mut subpath, &mut nodes);
            start = endpoint;
            subpath.push(endpoint);
            // Coordinate pairs after a moveto are linetos, per the spec.
            command = if relative { b'l' } else { b'L' };
        } else {
            subpath.push(endpoint);
        }
        current = endpoint;
    }

    flush(&mut subpath, &mut nodes);
    nodes
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

/// The value of `name="..."` on a single tag.
pub fn attribute(tag: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let mut rest = tag;
    while let Some(start) = rest.find(&needle) {
        // Guard against matching the tail of a longer attribute name.
        let preceding = rest[..start].chars().next_back();
        rest = &rest[start + needle.len()..];
        if matches!(preceding, Some(c) if c.is_alphanumeric() || c == '-' || c == ':') {
            continue;
        }
        return rest.find('"').map(|end| rest[..end].to_string());
    }
    None
}

fn attribute_number(tag: &str, name: &str) -> f64 {
    attribute(tag, name)
        .and_then(|value| scan_numbers(&value).first().copied())
        .unwrap_or(0.0)
}

/// Consume exactly `count` numbers, returning them and how many bytes that took.
fn take_numbers(text: &str, count: usize) -> Option<(Vec<f64>, usize)> {
    if count == 0 {
        return None;
    }
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut values = Vec::with_capacity(count);

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
        values.push(text[start..index].parse().ok()?);
    }

    Some((values, index))
}

pub fn scan_numbers(text: &str) -> Vec<f64> {
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

#[cfg(test)]
mod tests {
    use super::count;

    #[test]
    fn closing_convention_does_not_change_the_count() {
        // The same quadrilateral, written the two ways generators write it.
        let implicit = r#"<svg><path d="M0 0L10 0 10 10 0 10Z"/></svg>"#;
        let explicit = r#"<svg><path d="M0 0L10 0 10 10 0 10 0 0Z"/></svg>"#;
        assert_eq!(count(implicit), 4);
        assert_eq!(count(explicit), 4);
    }

    #[test]
    fn implicit_repetition_is_counted() {
        // One letter, two cubics.
        let svg = r#"<svg><path d="M0 0C1 1 2 2 3 3 4 4 5 5 6 6"/></svg>"#;
        assert_eq!(count(svg), 3);
    }

    #[test]
    fn relative_commands_track_the_current_point() {
        // potrace's dialect: a moveto then relative cubics closing on the start.
        let svg = r#"<svg><path d="M10 10 c5 0 5 5 0 5 c-5 0 -5 -5 0 -5 z"/></svg>"#;
        assert_eq!(count(svg), 2);
    }

    #[test]
    fn primitives_count_once() {
        let svg = r#"<svg><circle cx="5" cy="5" r="4"/><rect x="0" y="0" width="2" height="2"/></svg>"#;
        assert_eq!(count(svg), 2);
    }

    #[test]
    fn holes_are_separate_subpaths() {
        let svg = r#"<svg><path d="M0 0L10 0 10 10 0 10Z M2 2L8 2 8 8 2 8Z"/></svg>"#;
        assert_eq!(count(svg), 8);
    }
}
