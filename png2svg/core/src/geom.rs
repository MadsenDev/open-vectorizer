//! Basic 2D geometry helpers shared by the tracing and fitting stages.
//!
//! Everything downstream of contour extraction works in `f64` image coordinates
//! where `(0.0, 0.0)` is the top-left corner of the top-left pixel and pixel
//! `(x, y)` has its centre at `(x + 0.5, y + 0.5)`.

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

// Vector arithmetic is exposed as named methods rather than operator traits:
// the geometry code reads as chains (`b.sub(a).normalized().scale(k)`), which
// stays clearer than nested operator expressions once three or four terms are
// involved.
#[allow(clippy::should_implement_trait)]
impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    pub fn add(self, other: Point) -> Point {
        Point::new(self.x + other.x, self.y + other.y)
    }

    pub fn sub(self, other: Point) -> Point {
        Point::new(self.x - other.x, self.y - other.y)
    }

    pub fn scale(self, factor: f64) -> Point {
        Point::new(self.x * factor, self.y * factor)
    }

    pub fn dot(self, other: Point) -> f64 {
        self.x * other.x + self.y * other.y
    }

    /// 2D cross product magnitude (the z component of the 3D cross product).
    pub fn cross(self, other: Point) -> f64 {
        self.x * other.y - self.y * other.x
    }

    pub fn length_sq(self) -> f64 {
        self.x * self.x + self.y * self.y
    }

    pub fn length(self) -> f64 {
        self.length_sq().sqrt()
    }

    pub fn normalized(self) -> Point {
        let len = self.length();
        if len < 1e-12 {
            Point::new(0.0, 0.0)
        } else {
            Point::new(self.x / len, self.y / len)
        }
    }

    pub fn distance(self, other: Point) -> f64 {
        self.sub(other).length()
    }

    pub fn distance_sq(self, other: Point) -> f64 {
        self.sub(other).length_sq()
    }

    pub fn lerp(self, other: Point, t: f64) -> Point {
        Point::new(
            self.x + (other.x - self.x) * t,
            self.y + (other.y - self.y) * t,
        )
    }

    /// Rotate around the origin by `angle` radians.
    pub fn rotate(self, angle: f64) -> Point {
        let (sin, cos) = angle.sin_cos();
        Point::new(self.x * cos - self.y * sin, self.x * sin + self.y * cos)
    }

    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bounds {
    pub min_x: f64,
    pub min_y: f64,
    pub max_x: f64,
    pub max_y: f64,
}

impl Bounds {
    pub fn empty() -> Self {
        Self {
            min_x: f64::INFINITY,
            min_y: f64::INFINITY,
            max_x: f64::NEG_INFINITY,
            max_y: f64::NEG_INFINITY,
        }
    }

    pub fn of_points(points: &[Point]) -> Self {
        let mut bounds = Self::empty();
        for &point in points {
            bounds.add_point(point);
        }
        bounds
    }

    pub fn add_point(&mut self, point: Point) {
        self.min_x = self.min_x.min(point.x);
        self.min_y = self.min_y.min(point.y);
        self.max_x = self.max_x.max(point.x);
        self.max_y = self.max_y.max(point.y);
    }

    pub fn union(self, other: Bounds) -> Bounds {
        Bounds {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.min_x > self.max_x || self.min_y > self.max_y
    }

    pub fn expand(self, amount: f64) -> Bounds {
        if self.is_empty() {
            return self;
        }
        Bounds {
            min_x: self.min_x - amount,
            min_y: self.min_y - amount,
            max_x: self.max_x + amount,
            max_y: self.max_y + amount,
        }
    }

    pub fn width(&self) -> f64 {
        (self.max_x - self.min_x).max(0.0)
    }

    pub fn height(&self) -> f64 {
        (self.max_y - self.min_y).max(0.0)
    }

    /// True when `other` lies entirely inside `self`.
    pub fn contains_bounds(&self, other: &Bounds) -> bool {
        self.min_x <= other.min_x
            && self.min_y <= other.min_y
            && self.max_x >= other.max_x
            && self.max_y >= other.max_y
    }
}

/// Signed area of a polygon via the shoelace formula.
///
/// In the y-down image coordinate system a positive result means the vertices
/// run clockwise on screen.
pub fn signed_area(points: &[Point]) -> f64 {
    if points.len() < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    let mut previous = points[points.len() - 1];
    for &current in points {
        sum += previous.cross(current);
        previous = current;
    }
    sum * 0.5
}

/// Total length of the polyline, including the closing edge when `closed`.
pub fn polyline_length(points: &[Point], closed: bool) -> f64 {
    if points.len() < 2 {
        return 0.0;
    }
    let mut total = 0.0;
    for window in points.windows(2) {
        total += window[0].distance(window[1]);
    }
    if closed {
        total += points[points.len() - 1].distance(points[0]);
    }
    total
}

/// Crossing-number point-in-polygon test.
pub fn point_in_polygon(point: Point, polygon: &[Point]) -> bool {
    if polygon.len() < 3 {
        return false;
    }
    let mut inside = false;
    let mut j = polygon.len() - 1;
    for i in 0..polygon.len() {
        let a = polygon[i];
        let b = polygon[j];
        if (a.y > point.y) != (b.y > point.y) {
            let denominator = b.y - a.y;
            if denominator.abs() > 1e-12 {
                let x_at = a.x + (point.y - a.y) / denominator * (b.x - a.x);
                if point.x < x_at {
                    inside = !inside;
                }
            }
        }
        j = i;
    }
    inside
}

/// Perpendicular distance from `point` to the infinite line through `a` and `b`.
pub fn distance_to_line(point: Point, a: Point, b: Point) -> f64 {
    let direction = b.sub(a);
    let length = direction.length();
    if length < 1e-12 {
        return point.distance(a);
    }
    (direction.cross(point.sub(a)) / length).abs()
}

/// Intersection of the infinite lines `a0 + t * d0` and `a1 + s * d1`.
///
/// Returns `None` when the directions are parallel enough that the
/// intersection would be numerically meaningless.
pub fn line_intersection(a0: Point, d0: Point, a1: Point, d1: Point) -> Option<Point> {
    let denominator = d0.cross(d1);
    if denominator.abs() < 1e-9 {
        return None;
    }
    let t = a1.sub(a0).cross(d1) / denominator;
    let point = a0.add(d0.scale(t));
    if point.is_finite() {
        Some(point)
    } else {
        None
    }
}

/// Total-least-squares line fit: returns a point on the line and its unit
/// direction, together with the largest perpendicular residual.
pub fn fit_line(points: &[Point]) -> Option<(Point, Point, f64)> {
    if points.len() < 2 {
        return None;
    }

    let count = points.len() as f64;
    let mut centroid = Point::new(0.0, 0.0);
    for &point in points {
        centroid = centroid.add(point);
    }
    centroid = centroid.scale(1.0 / count);

    let mut sxx = 0.0;
    let mut sxy = 0.0;
    let mut syy = 0.0;
    for &point in points {
        let d = point.sub(centroid);
        sxx += d.x * d.x;
        sxy += d.x * d.y;
        syy += d.y * d.y;
    }

    // Principal direction of the 2x2 scatter matrix, in closed form.
    let theta = 0.5 * (2.0 * sxy).atan2(sxx - syy);
    let direction = Point::new(theta.cos(), theta.sin());

    let mut max_residual: f64 = 0.0;
    for &point in points {
        let residual = direction.cross(point.sub(centroid)).abs();
        max_residual = max_residual.max(residual);
    }

    Some((centroid, direction, max_residual))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_area_is_positive_for_clockwise_screen_polygons() {
        // y grows downward, so this winds clockwise on screen.
        let square = [
            Point::new(0.0, 0.0),
            Point::new(2.0, 0.0),
            Point::new(2.0, 2.0),
            Point::new(0.0, 2.0),
        ];
        assert!((signed_area(&square) - 4.0).abs() < 1e-9);

        let reversed: Vec<Point> = square.iter().rev().copied().collect();
        assert!((signed_area(&reversed) + 4.0).abs() < 1e-9);
    }

    #[test]
    fn point_in_polygon_handles_inside_and_outside() {
        let square = [
            Point::new(0.0, 0.0),
            Point::new(4.0, 0.0),
            Point::new(4.0, 4.0),
            Point::new(0.0, 4.0),
        ];
        assert!(point_in_polygon(Point::new(2.0, 2.0), &square));
        assert!(!point_in_polygon(Point::new(5.0, 2.0), &square));
        assert!(!point_in_polygon(Point::new(-1.0, 2.0), &square));
    }

    #[test]
    fn line_intersection_recovers_a_right_angle_corner() {
        let corner = line_intersection(
            Point::new(0.0, 5.0),
            Point::new(1.0, 0.0),
            Point::new(7.0, 0.0),
            Point::new(0.0, 1.0),
        )
        .expect("perpendicular lines intersect");
        assert!((corner.x - 7.0).abs() < 1e-9);
        assert!((corner.y - 5.0).abs() < 1e-9);
    }

    #[test]
    fn fit_line_recovers_a_diagonal() {
        let points: Vec<Point> = (0..10)
            .map(|i| Point::new(i as f64, i as f64 * 2.0))
            .collect();
        let (_, direction, residual) = fit_line(&points).expect("line fits");
        assert!(residual < 1e-9, "residual was {residual}");
        // Direction should be parallel to (1, 2).
        assert!(direction.cross(Point::new(1.0, 2.0)).abs() < 1e-9);
    }
}
