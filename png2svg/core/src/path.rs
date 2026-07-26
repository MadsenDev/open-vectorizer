//! Vector path representation produced by the fitting stage.

use crate::geom::{Bounds, Point};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Segment {
    Line { to: Point },
    Cubic { c1: Point, c2: Point, to: Point },
}

impl Segment {
    pub fn end_point(&self) -> Point {
        match *self {
            Segment::Line { to } => to,
            Segment::Cubic { to, .. } => to,
        }
    }
}

/// A closed outline made of line and cubic segments.
#[derive(Debug, Clone, PartialEq)]
pub struct Contour {
    pub start: Point,
    pub segments: Vec<Segment>,
}

impl Contour {
    pub fn from_polygon(points: &[Point]) -> Option<Contour> {
        let ring = strip_closing_duplicate(points);
        if ring.len() < 3 {
            return None;
        }
        let segments = ring[1..]
            .iter()
            .map(|&to| Segment::Line { to })
            .chain(std::iter::once(Segment::Line { to: ring[0] }))
            .collect();
        Some(Contour {
            start: ring[0],
            segments,
        })
    }

    pub fn node_count(&self) -> usize {
        self.segments.len()
    }

    /// Flatten to a polyline. `tolerance` is the maximum allowed deviation of a
    /// flattened cubic from the true curve, in pixels.
    pub fn flatten(&self, tolerance: f64) -> Vec<Point> {
        let mut points = vec![self.start];
        let mut current = self.start;
        for segment in &self.segments {
            match *segment {
                Segment::Line { to } => {
                    points.push(to);
                    current = to;
                }
                Segment::Cubic { c1, c2, to } => {
                    flatten_cubic(current, c1, c2, to, tolerance, &mut points);
                    current = to;
                }
            }
        }
        // The outline is closed; drop a duplicated wrap-around vertex.
        if points.len() > 1 && points[points.len() - 1].distance_sq(points[0]) < 1e-18 {
            points.pop();
        }
        points
    }

    pub fn bounds(&self) -> Bounds {
        // Control points bound the curve, which is all we need for tiling.
        let mut bounds = Bounds::empty();
        bounds.add_point(self.start);
        for segment in &self.segments {
            match *segment {
                Segment::Line { to } => bounds.add_point(to),
                Segment::Cubic { c1, c2, to } => {
                    bounds.add_point(c1);
                    bounds.add_point(c2);
                    bounds.add_point(to);
                }
            }
        }
        bounds
    }

    /// Exact signed area, via Green's theorem.
    ///
    /// For a cubic the integrand `x*y' - y*x'` is a degree-5 polynomial in `t`,
    /// which three-point Gauss-Legendre integrates exactly - so this is not an
    /// approximation and does not depend on any flattening tolerance.
    pub fn area(&self) -> f64 {
        let mut double_area = 0.0;
        let mut current = self.start;
        for segment in &self.segments {
            match *segment {
                Segment::Line { to } => {
                    double_area += current.cross(to);
                    current = to;
                }
                Segment::Cubic { c1, c2, to } => {
                    double_area += cubic_double_area(current, c1, c2, to);
                    current = to;
                }
            }
        }
        double_area * 0.5
    }
}

/// Contribution of one cubic to `2 * area`, i.e. the integral of
/// `x*y' - y*x'` over the segment.
fn cubic_double_area(p0: Point, p1: Point, p2: Point, p3: Point) -> f64 {
    // Three-point Gauss-Legendre on [0, 1].
    const OFFSET: f64 = 0.387_298_334_620_741_7; // 0.5 * sqrt(3/5)
    const NODES: [f64; 3] = [0.5 - OFFSET, 0.5, 0.5 + OFFSET];
    const WEIGHTS: [f64; 3] = [5.0 / 18.0, 8.0 / 18.0, 5.0 / 18.0];

    let mut total = 0.0;
    for index in 0..3 {
        let t = NODES[index];
        let position = eval_cubic(p0, p1, p2, p3, t);
        let velocity = eval_cubic_derivative(p0, p1, p2, p3, t);
        total += WEIGHTS[index] * position.cross(velocity);
    }
    total
}

/// The outer boundary (or a hole) of a shape, retaining primitive identity when
/// the geometry stage proved the region really is a circle, ellipse or rectangle.
#[derive(Debug, Clone, PartialEq)]
pub enum Outline {
    Path(Contour),
    Circle {
        center: Point,
        radius: f64,
    },
    Ellipse {
        center: Point,
        rx: f64,
        ry: f64,
        rotation: f64,
    },
    Rect {
        x: f64,
        y: f64,
        width: f64,
        height: f64,
    },
}

/// Control-point offset that makes four cubics approximate a circle to within
/// about 0.02% of the radius.
pub const KAPPA: f64 = 0.552_284_749_830_793_4;

impl Outline {
    pub fn to_contour(&self) -> Contour {
        match *self {
            Outline::Path(ref contour) => contour.clone(),
            Outline::Circle { center, radius } => {
                ellipse_contour(center, radius, radius, 0.0)
            }
            Outline::Ellipse {
                center,
                rx,
                ry,
                rotation,
            } => ellipse_contour(center, rx, ry, rotation),
            Outline::Rect {
                x,
                y,
                width,
                height,
            } => Contour {
                start: Point::new(x, y),
                segments: vec![
                    Segment::Line {
                        to: Point::new(x + width, y),
                    },
                    Segment::Line {
                        to: Point::new(x + width, y + height),
                    },
                    Segment::Line {
                        to: Point::new(x, y + height),
                    },
                    Segment::Line {
                        to: Point::new(x, y),
                    },
                ],
            },
        }
    }

    /// Number of on-curve nodes, used as the "how complex is this" metric when
    /// choosing between competing fits.
    pub fn node_count(&self) -> usize {
        match self {
            Outline::Path(contour) => contour.node_count(),
            // A primitive is a single element regardless of how many cubics it
            // would take to draw, so score it accordingly.
            Outline::Circle { .. } | Outline::Ellipse { .. } | Outline::Rect { .. } => 1,
        }
    }

    pub fn is_primitive(&self) -> bool {
        !matches!(self, Outline::Path(_))
    }

    pub fn bounds(&self) -> Bounds {
        match *self {
            Outline::Path(ref contour) => contour.bounds(),
            Outline::Circle { center, radius } => Bounds {
                min_x: center.x - radius,
                min_y: center.y - radius,
                max_x: center.x + radius,
                max_y: center.y + radius,
            },
            Outline::Ellipse { .. } => self.to_contour().bounds(),
            Outline::Rect {
                x,
                y,
                width,
                height,
            } => Bounds {
                min_x: x,
                min_y: y,
                max_x: x + width,
                max_y: y + height,
            },
        }
    }

    pub fn area(&self) -> f64 {
        match *self {
            Outline::Circle { radius, .. } => std::f64::consts::PI * radius * radius,
            Outline::Ellipse { rx, ry, .. } => std::f64::consts::PI * rx * ry,
            Outline::Rect { width, height, .. } => width * height,
            Outline::Path(ref contour) => contour.area().abs(),
        }
    }
}

/// One filled shape: an outer outline plus any holes cut out of it.
#[derive(Debug, Clone)]
pub struct Shape {
    pub color: [u8; 4],
    pub outer: Outline,
    pub holes: Vec<Outline>,
}

impl Shape {
    pub fn node_count(&self) -> usize {
        self.outer.node_count() + self.holes.iter().map(Outline::node_count).sum::<usize>()
    }

    pub fn bounds(&self) -> Bounds {
        self.outer.bounds()
    }

    pub fn area(&self) -> f64 {
        self.outer.area() - self.holes.iter().map(Outline::area).sum::<f64>()
    }

    /// Every outline as a contour, for rasterization.
    pub fn contours(&self) -> Vec<Contour> {
        let mut contours = vec![self.outer.to_contour()];
        contours.extend(self.holes.iter().map(Outline::to_contour));
        contours
    }
}

fn ellipse_contour(center: Point, rx: f64, ry: f64, rotation: f64) -> Contour {
    let ox = rx * KAPPA;
    let oy = ry * KAPPA;

    // Axis-aligned control net, then rotated into place.
    let local = |x: f64, y: f64| Point::new(x, y).rotate(rotation).add(center);

    let start = local(rx, 0.0);
    let segments = vec![
        Segment::Cubic {
            c1: local(rx, oy),
            c2: local(ox, ry),
            to: local(0.0, ry),
        },
        Segment::Cubic {
            c1: local(-ox, ry),
            c2: local(-rx, oy),
            to: local(-rx, 0.0),
        },
        Segment::Cubic {
            c1: local(-rx, -oy),
            c2: local(-ox, -ry),
            to: local(0.0, -ry),
        },
        Segment::Cubic {
            c1: local(ox, -ry),
            c2: local(rx, -oy),
            to: start,
        },
    ];

    Contour { start, segments }
}

fn strip_closing_duplicate(points: &[Point]) -> &[Point] {
    if points.len() >= 2 && points[points.len() - 1].distance_sq(points[0]) < 1e-18 {
        &points[..points.len() - 1]
    } else {
        points
    }
}

fn flatten_cubic(
    p0: Point,
    p1: Point,
    p2: Point,
    p3: Point,
    tolerance: f64,
    output: &mut Vec<Point>,
) {
    // Wang's formula for the number of segments needed to stay within
    // `tolerance` of a cubic.
    let a = p0.sub(p1.scale(2.0)).add(p2);
    let b = p1.sub(p2.scale(2.0)).add(p3);
    let max_second_derivative = a.length_sq().max(b.length_sq()).sqrt() * 6.0;
    let steps = if max_second_derivative <= 0.0 || tolerance <= 0.0 {
        1
    } else {
        ((max_second_derivative / (8.0 * tolerance)).sqrt().ceil() as usize).clamp(1, 512)
    };

    for step in 1..=steps {
        let t = step as f64 / steps as f64;
        output.push(eval_cubic(p0, p1, p2, p3, t));
    }
}

pub fn eval_cubic(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let mt = 1.0 - t;
    let a = mt * mt * mt;
    let b = 3.0 * mt * mt * t;
    let c = 3.0 * mt * t * t;
    let d = t * t * t;
    Point::new(
        p0.x * a + p1.x * b + p2.x * c + p3.x * d,
        p0.y * a + p1.y * b + p2.y * c + p3.y * d,
    )
}

pub fn eval_cubic_derivative(p0: Point, p1: Point, p2: Point, p3: Point, t: f64) -> Point {
    let mt = 1.0 - t;
    let a = 3.0 * mt * mt;
    let b = 6.0 * mt * t;
    let c = 3.0 * t * t;
    Point::new(
        (p1.x - p0.x) * a + (p2.x - p1.x) * b + (p3.x - p2.x) * c,
        (p1.y - p0.y) * a + (p2.y - p1.y) * b + (p3.y - p2.y) * c,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn circle_outline_flattens_to_the_right_radius() {
        let outline = Outline::Circle {
            center: Point::new(10.0, 10.0),
            radius: 5.0,
        };
        let points = outline.to_contour().flatten(0.01);
        assert!(points.len() > 16);
        for point in points {
            let radius = point.distance(Point::new(10.0, 10.0));
            assert!(
                (radius - 5.0).abs() < 0.01,
                "flattened radius drifted to {radius}"
            );
        }
    }

    #[test]
    fn circle_area_matches_pi_r_squared() {
        let outline = Outline::Circle {
            center: Point::new(0.0, 0.0),
            radius: 4.0,
        };
        let traced = outline.to_contour().area().abs();
        let expected = std::f64::consts::PI * 16.0;
        assert!(
            (traced - expected).abs() / expected < 1e-3,
            "area {traced} vs {expected}"
        );
    }

    #[test]
    fn rotated_ellipse_keeps_its_axes() {
        let outline = Outline::Ellipse {
            center: Point::new(0.0, 0.0),
            rx: 8.0,
            ry: 3.0,
            rotation: std::f64::consts::FRAC_PI_2,
        };
        let bounds = outline.to_contour().bounds();
        // Rotating by 90 degrees swaps the extents.
        assert!((bounds.width() - 6.0).abs() < 0.05, "{bounds:?}");
        assert!((bounds.height() - 16.0).abs() < 0.05, "{bounds:?}");
    }

    #[test]
    fn polygon_round_trips_through_contour() {
        let square = [
            Point::new(0.0, 0.0),
            Point::new(4.0, 0.0),
            Point::new(4.0, 4.0),
            Point::new(0.0, 4.0),
        ];
        let contour = Contour::from_polygon(&square).expect("square is a valid polygon");
        assert_eq!(contour.node_count(), 4);
        assert!((contour.area().abs() - 16.0).abs() < 1e-9);
    }
}
