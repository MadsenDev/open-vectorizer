//! SVG serialization.
//!
//! Primitives are written as real `<circle>`, `<ellipse>` and `<rect>` elements
//! rather than being flattened into path data, because that is what makes the
//! output editable: a designer can change a radius instead of dragging four
//! Bezier handles.

use std::fmt::Write;

use crate::path::{Contour, Outline, Segment, Shape};

/// A finished vector document.
#[derive(Debug, Clone)]
pub struct Document {
    pub width: u32,
    pub height: u32,
    /// Shapes in paint order: earlier shapes are painted first.
    pub shapes: Vec<Shape>,
}

/// Summary counts, used by the benchmark harness and worth exposing to callers
/// who want to compare engines.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub shapes: usize,
    pub nodes: usize,
    pub circles: usize,
    pub ellipses: usize,
    pub rects: usize,
}

impl Document {
    pub fn stats(&self) -> Stats {
        let mut stats = Stats {
            shapes: self.shapes.len(),
            ..Default::default()
        };
        for shape in &self.shapes {
            stats.nodes += shape.node_count();
            for outline in std::iter::once(&shape.outer).chain(shape.holes.iter()) {
                match outline {
                    Outline::Circle { .. } => stats.circles += 1,
                    Outline::Ellipse { .. } => stats.ellipses += 1,
                    Outline::Rect { .. } => stats.rects += 1,
                    Outline::Path(_) => {}
                }
            }
        }
        stats
    }

    pub fn to_svg(&self) -> String {
        write_svg(self)
    }
}

fn write_svg(document: &Document) -> String {
    let mut svg = String::with_capacity(1024 + document.shapes.len() * 128);
    let _ = writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {} {}\" width=\"{}\" height=\"{}\">",
        document.width, document.height, document.width, document.height
    );

    // Consecutive shapes sharing a fill are wrapped in one group, which keeps
    // paint order intact while still grouping by color the way an editor wants.
    let mut index = 0usize;
    while index < document.shapes.len() {
        let color = document.shapes[index].color;
        let mut end = index + 1;
        while end < document.shapes.len() && document.shapes[end].color == color {
            end += 1;
        }

        let hex = format!("#{:02x}{:02x}{:02x}", color[0], color[1], color[2]);
        if color[3] == 255 {
            let _ = writeln!(svg, "  <g fill=\"{hex}\">");
        } else {
            let _ = writeln!(
                svg,
                "  <g fill=\"{hex}\" fill-opacity=\"{}\">",
                number(color[3] as f64 / 255.0)
            );
        }

        for shape in &document.shapes[index..end] {
            write_shape(&mut svg, shape);
        }

        let _ = writeln!(svg, "  </g>");
        index = end;
    }

    svg.push_str("</svg>\n");
    svg
}

fn write_shape(svg: &mut String, shape: &Shape) {
    // A bare primitive can use its own element; once holes are involved the
    // shape has to become a path so even-odd fill can cut them out.
    if shape.holes.is_empty() {
        match shape.outer {
            Outline::Circle { center, radius } => {
                let _ = writeln!(
                    svg,
                    "    <circle cx=\"{}\" cy=\"{}\" r=\"{}\"/>",
                    number(center.x),
                    number(center.y),
                    number(radius)
                );
                return;
            }
            Outline::Ellipse {
                center,
                rx,
                ry,
                rotation,
            } => {
                let degrees = rotation.to_degrees();
                if degrees.abs() < 1e-6 {
                    let _ = writeln!(
                        svg,
                        "    <ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"/>",
                        number(center.x),
                        number(center.y),
                        number(rx),
                        number(ry)
                    );
                } else {
                    let _ = writeln!(
                        svg,
                        "    <ellipse cx=\"0\" cy=\"0\" rx=\"{}\" ry=\"{}\" transform=\"translate({} {}) rotate({})\"/>",
                        number(rx),
                        number(ry),
                        number(center.x),
                        number(center.y),
                        number(degrees)
                    );
                }
                return;
            }
            Outline::Rect {
                x,
                y,
                width,
                height,
            } => {
                let _ = writeln!(
                    svg,
                    "    <rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                    number(x),
                    number(y),
                    number(width),
                    number(height)
                );
                return;
            }
            Outline::Path(_) => {}
        }
    }

    let mut data = String::new();
    for contour in shape.contours() {
        if !data.is_empty() {
            data.push(' ');
        }
        data.push_str(&contour_to_path_data(&contour));
    }
    let _ = writeln!(svg, "    <path fill-rule=\"evenodd\" d=\"{data}\"/>");
}

/// Serialize one contour, omitting repeated command letters the way the SVG
/// grammar allows.
pub fn contour_to_path_data(contour: &Contour) -> String {
    let mut data = String::new();
    let _ = write!(
        data,
        "M{} {}",
        number(contour.start.x),
        number(contour.start.y)
    );

    let mut previous_command = ' ';
    let count = contour.segments.len();
    for (index, segment) in contour.segments.iter().enumerate() {
        // The closing Z implies the final line back to the start point.
        let is_last = index + 1 == count;
        match *segment {
            Segment::Line { to } => {
                if is_last && to.distance_sq(contour.start) < 1e-12 {
                    break;
                }
                if previous_command != 'L' {
                    data.push('L');
                    previous_command = 'L';
                } else {
                    data.push(' ');
                }
                let _ = write!(data, "{} {}", number(to.x), number(to.y));
            }
            Segment::Cubic { c1, c2, to } => {
                if previous_command != 'C' {
                    data.push('C');
                    previous_command = 'C';
                } else {
                    data.push(' ');
                }
                let _ = write!(
                    data,
                    "{} {} {} {} {} {}",
                    number(c1.x),
                    number(c1.y),
                    number(c2.x),
                    number(c2.y),
                    number(to.x),
                    number(to.y)
                );
            }
        }
    }

    data.push('Z');
    data
}

/// Compact fixed-point formatting: three decimals is finer than a thousandth of
/// a pixel, and trailing zeros are stripped so the output stays small.
fn number(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }

    let rounded = (value * 1000.0).round() / 1000.0;
    // Collapse negative zero, which would otherwise serialize as "-0".
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };

    if rounded.fract() == 0.0 && rounded.abs() < 1e15 {
        return format!("{}", rounded as i64);
    }

    let mut text = format!("{rounded:.3}");
    while text.ends_with('0') {
        text.pop();
    }
    if text.ends_with('.') {
        text.pop();
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;

    fn document_with(shapes: Vec<Shape>) -> Document {
        Document {
            width: 32,
            height: 32,
            shapes,
        }
    }

    #[test]
    fn a_bare_circle_becomes_a_circle_element() {
        let document = document_with(vec![Shape {
            color: [255, 0, 0, 255],
            outer: Outline::Circle {
                center: Point::new(16.0, 16.0),
                radius: 8.5,
            },
            holes: Vec::new(),
        }]);
        let svg = document.to_svg();
        assert!(
            svg.contains("<circle cx=\"16\" cy=\"16\" r=\"8.5\"/>"),
            "got {svg}"
        );
        assert!(!svg.contains("<path"));
    }

    #[test]
    fn a_circle_with_a_hole_becomes_an_even_odd_path() {
        let document = document_with(vec![Shape {
            color: [0, 0, 0, 255],
            outer: Outline::Circle {
                center: Point::new(16.0, 16.0),
                radius: 10.0,
            },
            holes: vec![Outline::Circle {
                center: Point::new(16.0, 16.0),
                radius: 5.0,
            }],
        }]);
        let svg = document.to_svg();
        assert!(svg.contains("fill-rule=\"evenodd\""), "got {svg}");
        assert_eq!(svg.matches('M').count(), 2, "outer and hole subpaths");
    }

    #[test]
    fn partial_alpha_becomes_fill_opacity() {
        let document = document_with(vec![Shape {
            color: [10, 20, 30, 128],
            outer: Outline::Rect {
                x: 0.0,
                y: 0.0,
                width: 4.0,
                height: 4.0,
            },
            holes: Vec::new(),
        }]);
        let svg = document.to_svg();
        assert!(svg.contains("fill=\"#0a141e\""), "got {svg}");
        assert!(svg.contains("fill-opacity=\"0.502\""), "got {svg}");
    }

    #[test]
    fn same_color_shapes_share_one_group() {
        let rect = |x: f64| Outline::Rect {
            x,
            y: 0.0,
            width: 2.0,
            height: 2.0,
        };
        let document = document_with(vec![
            Shape {
                color: [1, 2, 3, 255],
                outer: rect(0.0),
                holes: Vec::new(),
            },
            Shape {
                color: [1, 2, 3, 255],
                outer: rect(4.0),
                holes: Vec::new(),
            },
            Shape {
                color: [9, 9, 9, 255],
                outer: rect(8.0),
                holes: Vec::new(),
            },
        ]);
        let svg = document.to_svg();
        assert_eq!(svg.matches("<g fill=").count(), 2, "got {svg}");
    }

    #[test]
    fn path_data_omits_repeated_command_letters() {
        let contour = Contour {
            start: Point::new(0.0, 0.0),
            segments: vec![
                Segment::Line {
                    to: Point::new(4.0, 0.0),
                },
                Segment::Line {
                    to: Point::new(4.0, 4.0),
                },
                Segment::Line {
                    to: Point::new(0.0, 4.0),
                },
                Segment::Line {
                    to: Point::new(0.0, 0.0),
                },
            ],
        };
        // One L for the run, and the closing edge is implied by Z.
        assert_eq!(contour_to_path_data(&contour), "M0 0L4 0 4 4 0 4Z");
    }

    #[test]
    fn numbers_are_compact() {
        assert_eq!(number(16.0), "16");
        assert_eq!(number(16.5), "16.5");
        assert_eq!(number(-0.0), "0");
        assert_eq!(number(1.0 / 3.0), "0.333");
        assert_eq!(number(2.0004), "2");
    }

    #[test]
    fn stats_count_primitives() {
        let document = document_with(vec![Shape {
            color: [0, 0, 0, 255],
            outer: Outline::Circle {
                center: Point::new(1.0, 1.0),
                radius: 1.0,
            },
            holes: vec![Outline::Rect {
                x: 0.0,
                y: 0.0,
                width: 1.0,
                height: 1.0,
            }],
        }]);
        let stats = document.stats();
        assert_eq!(stats.shapes, 1);
        assert_eq!(stats.circles, 1);
        assert_eq!(stats.rects, 1);
    }
}
