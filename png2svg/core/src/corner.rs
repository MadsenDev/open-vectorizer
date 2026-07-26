//! Corner detection on traced contours.
//!
//! The naive test — "flag every vertex whose turn angle exceeds a threshold" —
//! cannot tell a genuine corner from a tightly curved arc, because a small
//! circle turns just as fast as a corner does. The discriminator used here is
//! scale behaviour: measure the turn over a short window and again over a
//! window twice as long. A corner concentrates all of its turning at one point,
//! so both windows report the same angle. An arc spreads its turning evenly, so
//! doubling the window doubles the angle.
//!
//! ```text
//! corner:  angle(d) / angle(2d) ~ 1.0
//! arc:     angle(d) / angle(2d) ~ 0.5
//! ```
//!
//! That ratio is scale invariant, so one threshold works for a 6px icon and a
//! 2000px hero mark alike.

use crate::geom::Point;

#[derive(Debug, Clone, Copy)]
pub struct CornerConfig {
    /// Half-width of the short measurement window, in pixels.
    pub scale: f64,
    /// Minimum turn angle, in radians, before a vertex is even considered.
    pub min_angle: f64,
    /// Minimum `angle(d) / angle(2d)` for a vertex to count as a corner.
    pub min_ratio: f64,
}

impl Default for CornerConfig {
    fn default() -> Self {
        Self {
            scale: 1.6,
            min_angle: 0.35,
            min_ratio: 0.72,
        }
    }
}

/// Indices into `ring` of the detected corners, in ascending order.
///
/// `ring` must be a closed contour given without a repeated final vertex.
pub fn detect_corners(ring: &[Point], config: &CornerConfig) -> Vec<usize> {
    let count = ring.len();
    if count < 8 {
        return Vec::new();
    }

    let cumulative = cumulative_lengths(ring);
    let perimeter = cumulative[count];
    if perimeter <= 0.0 {
        return Vec::new();
    }

    // A window must not wrap more than a third of the way around, or the
    // measurement stops being local.
    let scale = config.scale.min(perimeter / 6.0);
    if scale <= 1e-6 {
        return Vec::new();
    }

    let mut short_angle = vec![0.0f64; count];
    let mut long_angle = vec![0.0f64; count];
    for index in 0..count {
        short_angle[index] = turn_angle(ring, perimeter, index, scale);
        long_angle[index] = turn_angle(ring, perimeter, index, scale * 2.0);
    }

    let mut candidates: Vec<usize> = Vec::new();
    for index in 0..count {
        let short = short_angle[index];
        if short < config.min_angle {
            continue;
        }
        // A vanishing long-window angle means the two arms doubled back; treat
        // the ratio as satisfied rather than dividing by ~0.
        let ratio = if long_angle[index] > 1e-6 {
            short / long_angle[index]
        } else {
            1.0
        };
        if ratio >= config.min_ratio {
            candidates.push(index);
        }
    }

    suppress_non_maxima(&candidates, &short_angle, &cumulative, perimeter, scale)
}

/// Arc length from vertex 0 to each vertex, with the closing edge appended so
/// `cumulative[count]` is the full perimeter.
fn cumulative_lengths(ring: &[Point]) -> Vec<f64> {
    let count = ring.len();
    let mut cumulative = Vec::with_capacity(count + 1);
    cumulative.push(0.0);
    let mut total = 0.0;
    for index in 0..count {
        total += ring[index].distance(ring[(index + 1) % count]);
        cumulative.push(total);
    }
    cumulative
}

/// Absolute angle between the chord arriving at `index` and the chord leaving
/// it, each chord spanning `distance` of arc length.
fn turn_angle(ring: &[Point], perimeter: f64, index: usize, distance: f64) -> f64 {
    let behind = walk(ring, perimeter, index, distance, false);
    let ahead = walk(ring, perimeter, index, distance, true);
    if behind == index || ahead == index || behind == ahead {
        return 0.0;
    }

    let incoming = ring[index].sub(ring[behind]);
    let outgoing = ring[ahead].sub(ring[index]);
    if incoming.length_sq() < 1e-18 || outgoing.length_sq() < 1e-18 {
        return 0.0;
    }

    let cross = incoming.cross(outgoing);
    let dot = incoming.dot(outgoing);
    cross.atan2(dot).abs()
}

/// Index reached by walking `distance` of arc length from `index`.
///
/// Stops early at half the perimeter so a window can never wrap past the far
/// side of the contour and start measuring the shape from behind.
fn walk(
    ring: &[Point],
    perimeter: f64,
    index: usize,
    distance: f64,
    forward: bool,
) -> usize {
    let count = ring.len();
    let limit = distance.min(perimeter * 0.5);
    let mut travelled = 0.0;
    let mut current = index;

    for _ in 0..count {
        let next = if forward {
            (current + 1) % count
        } else {
            (current + count - 1) % count
        };
        travelled += ring[current].distance(ring[next]);
        current = next;
        if travelled >= limit {
            break;
        }
    }

    current
}

/// Keep only the strongest candidate within each `scale`-sized neighbourhood.
fn suppress_non_maxima(
    candidates: &[usize],
    strength: &[f64],
    cumulative: &[f64],
    perimeter: f64,
    scale: f64,
) -> Vec<usize> {
    if candidates.is_empty() {
        return Vec::new();
    }

    let arc_distance = |a: usize, b: usize| -> f64 {
        let raw = (cumulative[a] - cumulative[b]).abs();
        raw.min(perimeter - raw)
    };

    let mut ordered: Vec<usize> = candidates.to_vec();
    ordered.sort_by(|&a, &b| {
        strength[b]
            .partial_cmp(&strength[a])
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });

    let mut kept: Vec<usize> = Vec::new();
    for candidate in ordered {
        if kept
            .iter()
            .all(|&existing| arc_distance(candidate, existing) > scale)
        {
            kept.push(candidate);
        }
    }

    kept.sort_unstable();
    kept
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle_ring(radius: f64, samples: usize) -> Vec<Point> {
        (0..samples)
            .map(|index| {
                let angle = index as f64 / samples as f64 * std::f64::consts::TAU;
                Point::new(radius * angle.cos(), radius * angle.sin())
            })
            .collect()
    }

    fn square_ring(side: f64, per_side: usize) -> Vec<Point> {
        let mut ring = Vec::new();
        let corners = [
            (Point::new(0.0, 0.0), Point::new(side, 0.0)),
            (Point::new(side, 0.0), Point::new(side, side)),
            (Point::new(side, side), Point::new(0.0, side)),
            (Point::new(0.0, side), Point::new(0.0, 0.0)),
        ];
        for (from, to) in corners {
            for step in 0..per_side {
                let t = step as f64 / per_side as f64;
                ring.push(from.lerp(to, t));
            }
        }
        ring
    }

    #[test]
    fn a_square_has_exactly_four_corners() {
        let ring = square_ring(20.0, 20);
        let corners = detect_corners(&ring, &CornerConfig::default());
        assert_eq!(corners.len(), 4, "found corners at {corners:?}");
    }

    #[test]
    fn a_large_circle_has_no_corners() {
        let ring = circle_ring(40.0, 240);
        let corners = detect_corners(&ring, &CornerConfig::default());
        assert!(corners.is_empty(), "found spurious corners at {corners:?}");
    }

    #[test]
    fn a_small_circle_still_has_no_corners() {
        // The case a single-threshold detector gets wrong: a 4px circle turns
        // as fast as a corner does, but it turns *evenly*.
        let ring = circle_ring(4.0, 40);
        let corners = detect_corners(&ring, &CornerConfig::default());
        assert!(
            corners.is_empty(),
            "a small circle should not be read as a polygon, got {corners:?}"
        );
    }

    /// Densely sample a path so spacing matches what a real tracer produces
    /// (roughly one point per pixel of arc length).
    fn densify(vertices: &[Point], closed: bool) -> Vec<Point> {
        let mut ring = Vec::new();
        let count = vertices.len();
        let limit = if closed { count } else { count - 1 };
        for index in 0..limit {
            let from = vertices[index];
            let to = vertices[(index + 1) % count];
            let steps = (from.distance(to).ceil() as usize).max(1);
            for step in 0..steps {
                ring.push(from.lerp(to, step as f64 / steps as f64));
            }
        }
        ring
    }

    #[test]
    fn a_rounded_rectangle_reads_as_smooth() {
        // Rounded corners of radius 5px joined by straight sides: the corners
        // are arcs, and nothing here should be flagged.
        let radius = 5.0;
        let straight = 30.0;
        let mut vertices = Vec::new();
        let arc_centres = [
            (Point::new(straight, straight), 0.0),
            (Point::new(0.0, straight), std::f64::consts::FRAC_PI_2),
            (Point::new(0.0, 0.0), std::f64::consts::PI),
            (Point::new(straight, 0.0), 3.0 * std::f64::consts::FRAC_PI_2),
        ];
        for (centre, start) in arc_centres {
            for step in 0..=24 {
                let angle = start + step as f64 / 24.0 * std::f64::consts::FRAC_PI_2;
                vertices.push(Point::new(
                    centre.x + radius * angle.cos(),
                    centre.y + radius * angle.sin(),
                ));
            }
        }
        // densify fills in the straight sides between consecutive arcs.
        let ring = densify(&vertices, true);

        let corners = detect_corners(&ring, &CornerConfig::default());
        assert!(
            corners.is_empty(),
            "rounded corners should stay smooth, got {corners:?}"
        );
    }

    #[test]
    fn a_teardrop_finds_its_single_sharp_point() {
        // A proper teardrop: an arc plus the two tangent lines drawn to an
        // external point. The tangency joins are smooth by construction, so the
        // tip is the only corner in the shape.
        let radius = 20.0;
        let tip = Point::new(46.0, 0.0);
        let alpha = (radius / tip.x).acos();
        let samples = 220;

        let mut vertices = Vec::new();
        for step in 0..=samples {
            let angle =
                alpha + step as f64 / samples as f64 * (std::f64::consts::TAU - 2.0 * alpha);
            vertices.push(Point::new(radius * angle.cos(), radius * angle.sin()));
        }
        vertices.push(tip);
        let ring = densify(&vertices, true);

        let corners = detect_corners(&ring, &CornerConfig::default());
        assert_eq!(corners.len(), 1, "found corners at {corners:?}");
        let corner = ring[corners[0]];
        assert!(
            corner.distance(tip) < 1.5,
            "corner landed at {corner:?}, expected near {tip:?}"
        );
    }

    #[test]
    fn a_triangle_has_three_corners() {
        let mut ring = Vec::new();
        let vertices = [
            Point::new(0.0, 0.0),
            Point::new(30.0, 5.0),
            Point::new(12.0, 28.0),
        ];
        for index in 0..3 {
            let from = vertices[index];
            let to = vertices[(index + 1) % 3];
            for step in 0..25 {
                ring.push(from.lerp(to, step as f64 / 25.0));
            }
        }
        let corners = detect_corners(&ring, &CornerConfig::default());
        assert_eq!(corners.len(), 3, "found corners at {corners:?}");
    }
}
