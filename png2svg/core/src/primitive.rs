//! Primitive recovery.
//!
//! A logo's circle was a circle before someone rasterized it. Fitting Beziers
//! to the trace of one produces a shape that is *close* to round but wobbles,
//! and no amount of node reduction makes it exact. Detecting the primitive and
//! emitting `<circle>` instead gives geometry that is exactly right, editable,
//! and a single element.
//!
//! Every fit here reports its own worst-case error so the caller can decide
//! whether to accept it, rather than the decision being buried in a threshold.

use crate::geom::{signed_area, Point};
use crate::path::{Outline, Segment};

#[derive(Debug, Clone, Copy)]
pub struct CircleFit {
    pub center: Point,
    pub radius: f64,
    pub max_error: f64,
}

#[derive(Debug, Clone, Copy)]
pub struct EllipseFit {
    pub center: Point,
    pub rx: f64,
    pub ry: f64,
    pub rotation: f64,
    pub max_error: f64,
}

/// Least-squares circle fit.
///
/// Starts from Kasa's algebraic solution and polishes it with Landau's fixed
/// point iteration, which converges on the true geometric least-squares circle
/// (the algebraic fit alone is biased when the arc does not span the full
/// circle).
pub fn fit_circle(ring: &[Point]) -> Option<CircleFit> {
    if ring.len() < 8 {
        return None;
    }

    let count = ring.len() as f64;
    let mut mean = Point::new(0.0, 0.0);
    for &point in ring {
        mean = mean.add(point);
    }
    mean = mean.scale(1.0 / count);

    // Kasa: solve the linear system for the algebraic circle, centred on the
    // data to keep the normal equations well conditioned.
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    let mut syy = 0.0;
    let mut sxz = 0.0;
    let mut syz = 0.0;
    for &point in ring {
        let d = point.sub(mean);
        let z = d.x * d.x + d.y * d.y;
        sxx += d.x * d.x;
        sxy += d.x * d.y;
        syy += d.y * d.y;
        sxz += d.x * z;
        syz += d.y * z;
    }

    let determinant = sxx * syy - sxy * sxy;
    if determinant.abs() < 1e-12 {
        return None;
    }
    let cx = (sxz * syy - syz * sxy) / (2.0 * determinant);
    let cy = (syz * sxx - sxz * sxy) / (2.0 * determinant);
    let mut center = mean.add(Point::new(cx, cy));

    // Landau refinement: alternate the closed-form radius with the closed-form
    // centre until it settles.
    let mut radius = 0.0;
    for _ in 0..48 {
        let mut distance_sum = 0.0;
        let mut unit_sum = Point::new(0.0, 0.0);
        for &point in ring {
            let offset = center.sub(point);
            let distance = offset.length();
            if distance < 1e-12 {
                return None;
            }
            distance_sum += distance;
            unit_sum = unit_sum.add(offset.scale(1.0 / distance));
        }
        radius = distance_sum / count;
        let updated = mean.add(unit_sum.scale(radius / count));
        let shift = updated.distance(center);
        center = updated;
        if shift < 1e-10 {
            break;
        }
    }

    if !(radius.is_finite() && radius > 0.0 && center.is_finite()) {
        return None;
    }

    let mut max_error: f64 = 0.0;
    for &point in ring {
        max_error = max_error.max((point.distance(center) - radius).abs());
    }

    // A closed circle has to be swept all the way round; an arc that happens to
    // be circular is not a `<circle>`.
    if !spans_full_turn(ring, center) {
        return None;
    }

    Some(CircleFit {
        center,
        radius,
        max_error,
    })
}

/// Ellipse fit by area-moment matching.
///
/// A filled ellipse's covariance matrix is `R * diag(a^2/4, b^2/4) * R^T`, so
/// the polygon's second moments determine the ellipse exactly. That avoids the
/// generalized eigenproblem a direct conic fit would need, and it is far more
/// stable on a closed traced outline.
pub fn fit_ellipse(ring: &[Point]) -> Option<EllipseFit> {
    if ring.len() < 12 {
        return None;
    }

    let moments = polygon_moments(ring)?;
    let (cxx, cyy, cxy) = (moments.cxx, moments.cyy, moments.cxy);

    // Eigen-decomposition of the 2x2 covariance, in closed form.
    let half_trace = (cxx + cyy) * 0.5;
    let determinant = cxx * cyy - cxy * cxy;
    let gap_sq = half_trace * half_trace - determinant;
    if gap_sq < 0.0 {
        return None;
    }
    let gap = gap_sq.sqrt();
    let lambda_major = half_trace + gap;
    let lambda_minor = half_trace - gap;
    if lambda_minor <= 1e-12 {
        return None;
    }

    let rx = 2.0 * lambda_major.sqrt();
    let ry = 2.0 * lambda_minor.sqrt();
    if !(rx.is_finite() && ry.is_finite()) || ry <= 0.0 {
        return None;
    }

    // Eigenvector for the major axis; pick the better-conditioned expression.
    let rotation = if cxy.abs() > 1e-12 {
        (lambda_major - cxx).atan2(cxy)
    } else if cxx >= cyy {
        0.0
    } else {
        std::f64::consts::FRAC_PI_2
    };

    let center = moments.centroid;
    let (sin, cos) = rotation.sin_cos();

    // First-order distance from each point to the ellipse: |F| / |grad F| for
    // the implicit form, which is accurate for points close to the curve.
    let mut max_error: f64 = 0.0;
    for &point in ring {
        let offset = point.sub(center);
        let local_x = offset.x * cos + offset.y * sin;
        let local_y = -offset.x * sin + offset.y * cos;

        let value = (local_x / rx).powi(2) + (local_y / ry).powi(2) - 1.0;
        let gradient = Point::new(
            2.0 * local_x / (rx * rx),
            2.0 * local_y / (ry * ry),
        );
        let gradient_length = gradient.length();
        if gradient_length < 1e-12 {
            return None;
        }
        max_error = max_error.max((value / gradient_length).abs());
    }

    Some(EllipseFit {
        center,
        rx,
        ry,
        rotation,
        max_error,
    })
}

struct Moments {
    centroid: Point,
    cxx: f64,
    cyy: f64,
    cxy: f64,
}

/// Area, centroid and central second moments of a simple polygon, via Green's
/// theorem.
fn polygon_moments(ring: &[Point]) -> Option<Moments> {
    let count = ring.len();
    if count < 3 {
        return None;
    }

    let area = signed_area(ring);
    if area.abs() < 1e-9 {
        return None;
    }

    let mut cx = 0.0;
    let mut cy = 0.0;
    let mut ixx = 0.0;
    let mut iyy = 0.0;
    let mut ixy = 0.0;

    for index in 0..count {
        let a = ring[index];
        let b = ring[(index + 1) % count];
        let cross = a.x * b.y - b.x * a.y;

        cx += (a.x + b.x) * cross;
        cy += (a.y + b.y) * cross;
        ixx += (a.x * a.x + a.x * b.x + b.x * b.x) * cross;
        iyy += (a.y * a.y + a.y * b.y + b.y * b.y) * cross;
        ixy += (a.x * b.y + 2.0 * a.x * a.y + 2.0 * b.x * b.y + b.x * a.y) * cross;
    }

    let centroid = Point::new(cx / (6.0 * area), cy / (6.0 * area));
    // Second moments about the origin, then shifted to the centroid.
    let ixx = ixx / 12.0 / area - centroid.x * centroid.x;
    let iyy = iyy / 12.0 / area - centroid.y * centroid.y;
    let ixy = ixy / 24.0 / area - centroid.x * centroid.y;

    if !(ixx.is_finite() && iyy.is_finite() && ixy.is_finite()) {
        return None;
    }

    Some(Moments {
        centroid,
        cxx: ixx,
        cyy: iyy,
        cxy: ixy,
    })
}

/// True when the ring's points wrap all the way around `center` without a large
/// angular gap.
fn spans_full_turn(ring: &[Point], center: Point) -> bool {
    let mut angles: Vec<f64> = ring
        .iter()
        .map(|point| {
            let offset = point.sub(center);
            offset.y.atan2(offset.x)
        })
        .collect();
    angles.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let mut largest_gap: f64 = 0.0;
    for window in angles.windows(2) {
        largest_gap = largest_gap.max(window[1] - window[0]);
    }
    // Wrap-around gap.
    if let (Some(&first), Some(&last)) = (angles.first(), angles.last()) {
        largest_gap = largest_gap.max(first + std::f64::consts::TAU - last);
    }

    largest_gap < 0.6
}

/// Recognise an axis-aligned rectangle in an already-fitted contour.
///
/// Runs after fitting rather than before, so it only accepts shapes the corner
/// and line stages already agreed are four straight sides.
pub fn detect_rect(start: Point, segments: &[Segment], tolerance: f64) -> Option<Outline> {
    if segments.len() != 4 {
        return None;
    }
    if !segments
        .iter()
        .all(|segment| matches!(segment, Segment::Line { .. }))
    {
        return None;
    }

    let mut vertices = vec![start];
    for segment in &segments[..3] {
        vertices.push(segment.end_point());
    }
    // The final segment must close the loop.
    if segments[3].end_point().distance(start) > tolerance.max(1e-6) {
        return None;
    }

    // Alternating horizontal and vertical edges, each within tolerance of axis
    // aligned. The traced contour can start at any of the four corners, so the
    // parity is taken from the first edge rather than assumed.
    let first = vertices[1].sub(vertices[0]);
    let first_is_horizontal = first.y.abs() <= first.x.abs();

    for index in 0..4 {
        let a = vertices[index];
        let b = vertices[(index + 1) % 4];
        let dx = (b.x - a.x).abs();
        let dy = (b.y - a.y).abs();
        let expects_horizontal = (index % 2 == 0) == first_is_horizontal;
        let (along, across) = if expects_horizontal { (dx, dy) } else { (dy, dx) };
        if across > tolerance || along <= tolerance {
            return None;
        }
    }

    let min_x = vertices.iter().map(|p| p.x).fold(f64::INFINITY, f64::min);
    let max_x = vertices
        .iter()
        .map(|p| p.x)
        .fold(f64::NEG_INFINITY, f64::max);
    let min_y = vertices.iter().map(|p| p.y).fold(f64::INFINITY, f64::min);
    let max_y = vertices
        .iter()
        .map(|p| p.y)
        .fold(f64::NEG_INFINITY, f64::max);

    Some(Outline::Rect {
        x: min_x,
        y: min_y,
        width: max_x - min_x,
        height: max_y - min_y,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_circle(center: Point, radius: f64, samples: usize) -> Vec<Point> {
        (0..samples)
            .map(|index| {
                let angle = index as f64 / samples as f64 * std::f64::consts::TAU;
                Point::new(
                    center.x + radius * angle.cos(),
                    center.y + radius * angle.sin(),
                )
            })
            .collect()
    }

    fn sample_ellipse(
        center: Point,
        rx: f64,
        ry: f64,
        rotation: f64,
        samples: usize,
    ) -> Vec<Point> {
        (0..samples)
            .map(|index| {
                let angle = index as f64 / samples as f64 * std::f64::consts::TAU;
                Point::new(rx * angle.cos(), ry * angle.sin())
                    .rotate(rotation)
                    .add(center)
            })
            .collect()
    }

    #[test]
    fn circle_fit_recovers_centre_and_radius() {
        let ring = sample_circle(Point::new(12.5, -3.25), 17.75, 96);
        let fit = fit_circle(&ring).expect("a circle should fit a circle");
        assert!((fit.center.x - 12.5).abs() < 1e-6, "{fit:?}");
        assert!((fit.center.y + 3.25).abs() < 1e-6, "{fit:?}");
        assert!((fit.radius - 17.75).abs() < 1e-6, "{fit:?}");
        assert!(fit.max_error < 1e-6, "{fit:?}");
    }

    #[test]
    fn circle_fit_survives_sub_pixel_noise() {
        // Deterministic pseudo-noise at the scale a real tracer produces.
        let mut ring = sample_circle(Point::new(40.0, 40.0), 25.0, 160);
        for (index, point) in ring.iter_mut().enumerate() {
            let wobble = ((index * 7919) % 17) as f64 / 17.0 - 0.5;
            let radial = point.sub(Point::new(40.0, 40.0)).normalized();
            *point = point.add(radial.scale(wobble * 0.08));
        }
        let fit = fit_circle(&ring).expect("noisy circle still fits");
        assert!((fit.radius - 25.0).abs() < 0.05, "{fit:?}");
        assert!(fit.max_error < 0.1, "{fit:?}");
    }

    #[test]
    fn circle_fit_rejects_a_square() {
        let mut ring = Vec::new();
        for index in 0..4 {
            let corners = [
                Point::new(0.0, 0.0),
                Point::new(20.0, 0.0),
                Point::new(20.0, 20.0),
                Point::new(0.0, 20.0),
            ];
            let from = corners[index];
            let to = corners[(index + 1) % 4];
            for step in 0..20 {
                ring.push(from.lerp(to, step as f64 / 20.0));
            }
        }
        let fit = fit_circle(&ring).expect("a square still admits a best-fit circle");
        // The fit exists but its error must be far too large to accept.
        assert!(fit.max_error > 1.5, "{fit:?}");
    }

    #[test]
    fn circle_fit_rejects_a_half_arc() {
        let ring: Vec<Point> = sample_circle(Point::new(0.0, 0.0), 10.0, 120)
            .into_iter()
            .filter(|point| point.y >= 0.0)
            .collect();
        assert!(
            fit_circle(&ring).is_none(),
            "a half arc is not a closed circle"
        );
    }

    #[test]
    fn ellipse_fit_recovers_axes_and_rotation() {
        let rotation = 0.7;
        let ring = sample_ellipse(Point::new(5.0, 9.0), 20.0, 8.0, rotation, 240);
        let fit = fit_ellipse(&ring).expect("an ellipse should fit an ellipse");

        assert!((fit.center.x - 5.0).abs() < 0.02, "{fit:?}");
        assert!((fit.center.y - 9.0).abs() < 0.02, "{fit:?}");
        assert!((fit.rx - 20.0).abs() < 0.05, "{fit:?}");
        assert!((fit.ry - 8.0).abs() < 0.05, "{fit:?}");

        // Rotation is only defined modulo pi for an ellipse.
        let difference = (fit.rotation - rotation).rem_euclid(std::f64::consts::PI);
        let difference = difference.min(std::f64::consts::PI - difference);
        assert!(difference < 0.01, "rotation off by {difference}: {fit:?}");
        assert!(fit.max_error < 0.05, "{fit:?}");
    }

    #[test]
    fn ellipse_fit_reports_large_error_for_a_rectangle() {
        let mut ring = Vec::new();
        let corners = [
            Point::new(0.0, 0.0),
            Point::new(40.0, 0.0),
            Point::new(40.0, 16.0),
            Point::new(0.0, 16.0),
        ];
        for index in 0..4 {
            let from = corners[index];
            let to = corners[(index + 1) % 4];
            for step in 0..30 {
                ring.push(from.lerp(to, step as f64 / 30.0));
            }
        }
        let fit = fit_ellipse(&ring).expect("moments always produce some ellipse");
        assert!(fit.max_error > 1.0, "{fit:?}");
    }

    #[test]
    fn rect_detection_accepts_an_axis_aligned_square() {
        let start = Point::new(3.0, 4.0);
        let segments = vec![
            Segment::Line {
                to: Point::new(13.0, 4.0),
            },
            Segment::Line {
                to: Point::new(13.0, 20.0),
            },
            Segment::Line {
                to: Point::new(3.0, 20.0),
            },
            Segment::Line { to: start },
        ];
        let outline = detect_rect(start, &segments, 0.3).expect("should detect a rect");
        match outline {
            Outline::Rect {
                x,
                y,
                width,
                height,
            } => {
                assert!((x - 3.0).abs() < 1e-9);
                assert!((y - 4.0).abs() < 1e-9);
                assert!((width - 10.0).abs() < 1e-9);
                assert!((height - 16.0).abs() < 1e-9);
            }
            other => panic!("expected a rect, got {other:?}"),
        }
    }

    #[test]
    fn rect_detection_rejects_a_rotated_square() {
        let start = Point::new(0.0, 0.0);
        let segments = vec![
            Segment::Line {
                to: Point::new(10.0, 3.0),
            },
            Segment::Line {
                to: Point::new(7.0, 13.0),
            },
            Segment::Line {
                to: Point::new(-3.0, 10.0),
            },
            Segment::Line { to: start },
        ];
        assert!(detect_rect(start, &segments, 0.3).is_none());
    }
}
