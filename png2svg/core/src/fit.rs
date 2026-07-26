//! Curve fitting.
//!
//! Contours arrive as dense polylines. This module turns them into the smallest
//! set of line and cubic segments that stays within a stated error budget,
//! using Schneider's least-squares cubic fit with Newton-Raphson
//! reparameterization (Graphics Gems, 1990).
//!
//! The piece that matters for logo quality is the assembly around corners.
//! Marching squares chamfers a sharp corner: a 90 degree turn comes back as two
//! 45 degree steps straddling the true vertex, which no amount of curve fitting
//! will sharpen back up. So straight runs are fitted from their *interiors*,
//! with the chamfered ends trimmed away, and the corner is then recovered
//! exactly by intersecting the two fitted lines.

use crate::geom::{fit_line, line_intersection, Point};
use crate::path::{eval_cubic, eval_cubic_derivative, Contour, Segment};

const MAX_NEWTON_ITERATIONS: usize = 4;

/// How far back from each end of a straight run to start trusting the points,
/// in pixels. Anything closer to the corner may be sitting on a chamfer.
const CORNER_TRIM: f64 = 1.25;

/// The tracer's own precision, in pixels.
///
/// Contours recovered from anti-aliased coverage carry a little noise, so no
/// straightness test should demand better agreement than this — however tight an
/// error budget the caller asked for. Without a floor, a very tight tolerance
/// stops recognising straight sides at all and a polygon comes back as hundreds
/// of cubics: worse geometry *and* worse accuracy than the same shape fitted
/// loosely.
const TRACER_NOISE: f64 = 0.12;

/// Fit a closed contour that has no corners: one smooth loop.
pub fn fit_closed_smooth(ring: &[Point], tolerance: f64) -> Vec<Segment> {
    let count = ring.len();
    if count < 4 {
        return polygon_segments(ring);
    }

    // Fit as an open curve that returns to its start, constraining both ends to
    // the same tangent so the seam stays smooth.
    let mut points: Vec<Point> = ring.to_vec();
    points.push(ring[0]);

    let seam_tangent = ring[1].sub(ring[count - 1]).normalized();
    if seam_tangent.length_sq() < 1e-18 {
        return polygon_segments(ring);
    }

    let mut segments = Vec::new();
    fit_cubic(
        &points,
        seam_tangent,
        seam_tangent.scale(-1.0),
        tolerance,
        &mut segments,
    );
    segments
}

/// Fit a closed contour.
///
/// Breaks the ring at corners *and* at the ends of long straight runs, so a
/// shape that mixes lines with arcs — a rounded rectangle, a shield, a tab —
/// gets exact lines where it is straight and curves only where it curves. A
/// straight run bounded by smooth junctions is joined to its neighbouring
/// curves with matching tangents, so nothing kinks.
pub fn fit_contour(ring: &[Point], corners: &[usize], tolerance: f64) -> Contour {
    let breaks = build_breaks(ring, corners, tolerance);

    let fitted = if breaks.is_empty() {
        Contour {
            start: ring[0],
            segments: fit_closed_smooth(ring, tolerance),
        }
    } else {
        fit_segmented(ring, &breaks, tolerance)
    };
    straighten(fitted, tolerance)
}

/// A point where the contour changes character.
#[derive(Debug, Clone, Copy)]
struct Break {
    index: usize,
    /// True at a corner, where the tangent is allowed to jump.
    sharp: bool,
}

fn build_breaks(ring: &[Point], corners: &[usize], tolerance: f64) -> Vec<Break> {
    let mut breaks: Vec<Break> = corners
        .iter()
        .map(|&index| Break { index, sharp: true })
        .collect();

    let cumulative = arc_lengths(ring);
    let perimeter = *cumulative.last().unwrap_or(&0.0);
    if perimeter <= 0.0 {
        return breaks;
    }

    // A straight-run boundary this close to a corner is the same transition
    // seen twice; the corner already marks it.
    let merge_radius = 3.0_f64.max(tolerance * 6.0);
    let arc_distance = |a: usize, b: usize| -> f64 {
        let raw = (cumulative[a] - cumulative[b]).abs();
        raw.min(perimeter - raw)
    };

    for (start, end) in find_straight_runs(ring, tolerance) {
        for candidate in [start, end] {
            if breaks
                .iter()
                .all(|existing| arc_distance(existing.index, candidate) > merge_radius)
            {
                breaks.push(Break {
                    index: candidate,
                    sharp: false,
                });
            }
        }
    }

    breaks.sort_unstable_by_key(|item| item.index);
    breaks.dedup_by_key(|item| item.index);
    // A single break cannot define a span; fall back to the smooth closed fit.
    if breaks.len() < 2 {
        breaks.clear();
    }
    breaks
}

fn arc_lengths(ring: &[Point]) -> Vec<f64> {
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

/// Running second moments, used to test straightness in constant time.
#[derive(Default, Clone, Copy)]
struct Moments {
    n: f64,
    sx: f64,
    sy: f64,
    sxx: f64,
    sxy: f64,
    syy: f64,
}

impl Moments {
    fn add(&mut self, point: Point) {
        self.n += 1.0;
        self.sx += point.x;
        self.sy += point.y;
        self.sxx += point.x * point.x;
        self.sxy += point.x * point.y;
        self.syy += point.y * point.y;
    }

    /// Root-mean-square perpendicular distance to the best-fit line.
    ///
    /// The sum of squared perpendicular distances is the smaller eigenvalue of
    /// the centred scatter matrix, which is available in closed form — so
    /// extending a run costs nothing.
    fn rms(&self) -> f64 {
        if self.n < 3.0 {
            return 0.0;
        }
        let cxx = self.sxx - self.sx * self.sx / self.n;
        let cyy = self.syy - self.sy * self.sy / self.n;
        let cxy = self.sxy - self.sx * self.sy / self.n;

        let half_trace = (cxx + cyy) * 0.5;
        let determinant = cxx * cyy - cxy * cxy;
        let gap = (half_trace * half_trace - determinant).max(0.0).sqrt();
        let smaller = (half_trace - gap).max(0.0);
        (smaller / self.n).sqrt()
    }
}

/// Straightness limit for a run, as RMS perpendicular deviation in pixels.
///
/// Deliberately *not* derived from the caller's tolerance. A side that was
/// straight in the original artwork traces back straight to within the tracer's
/// own noise — a few hundredths of a pixel — however loose an error budget the
/// caller asked for. Testing against that noise floor instead is what separates
/// a real straight side from a stretch of a large circle, which is straight to
/// within any generous tolerance but not to within this one.
const STRAIGHT_RUN_RMS: f64 = 0.08;

/// Shortest run worth reporting, in pixels, and as a fraction of the perimeter.
///
/// Even at the noise floor, a big enough circle has stretches that pass the RMS
/// test — radius 300 yields about 17px. Requiring a run to also cover a share of
/// the perimeter rejects those while keeping the genuinely straight sides of
/// rounded rectangles, capsules and shields.
const MIN_RUN_PIXELS: f64 = 8.0;
const MIN_RUN_FRACTION: f64 = 0.06;

/// How far a point may sit off a run's own line and still count as part of it.
///
/// Much tighter than the run-acceptance test, because this decides *where the
/// straight part ends*. Where a straight side meets an arc of radius `r`, the
/// arc pulls away from the tangent line as `L^2 / 2r`, so this limit places the
/// junction within a pixel or two of the true tangent point. Without it a run
/// bleeds several pixels into the arc, and the arc fit then has to be split to
/// absorb a tangent constraint taken from the wrong place.
const RUN_END_LIMIT: f64 = 0.04;

/// Shrink a run to the part that is genuinely straight.
///
/// The line is taken from the middle of the run, which is straight by
/// construction, and the ends are then walked inward until they agree with it.
fn trim_run(ring: &[Point], start: usize, end: usize) -> (usize, usize) {
    let span = end - start;
    if span < 6 {
        return (start, end);
    }

    let quarter = span / 4;
    let core = &ring[(start + quarter)..=(end - quarter)];
    let Some((on_line, direction, _)) = fit_line(core) else {
        return (start, end);
    };

    let offset = |index: usize| -> f64 { direction.cross(ring[index].sub(on_line)).abs() };

    let mut trimmed_start = start + quarter;
    while trimmed_start > start && offset(trimmed_start - 1) <= RUN_END_LIMIT {
        trimmed_start -= 1;
    }

    let mut trimmed_end = end - quarter;
    while trimmed_end < end && offset(trimmed_end + 1) <= RUN_END_LIMIT {
        trimmed_end += 1;
    }

    (trimmed_start, trimmed_end)
}

/// Maximal spans of the ring that are straight.
fn find_straight_runs(ring: &[Point], tolerance: f64) -> Vec<(usize, usize)> {
    let count = ring.len();
    if count < 12 {
        return Vec::new();
    }

    let perimeter = crate::geom::polyline_length(ring, true);
    if perimeter <= 0.0 {
        return Vec::new();
    }
    let minimum_length = MIN_RUN_PIXELS.max(perimeter * MIN_RUN_FRACTION);
    // Whether a side is straight is a property of the artwork, not of the error
    // budget, so this does not scale with `tolerance`.
    let _ = tolerance;
    let rms_limit = STRAIGHT_RUN_RMS;

    let mut runs: Vec<(usize, usize)> = Vec::new();
    let mut start = 0usize;
    while start + 1 < count {
        let mut moments = Moments::default();
        moments.add(ring[start]);
        let mut furthest = start;

        for (offset, &point) in ring[start + 1..].iter().enumerate() {
            let mut extended = moments;
            extended.add(point);
            if extended.rms() > rms_limit {
                break;
            }
            moments = extended;
            furthest = start + 1 + offset;
        }

        if furthest > start {
            let length = crate::geom::polyline_length(&ring[start..=furthest], false);
            if length >= minimum_length {
                runs.push(trim_run(ring, start, furthest));
            }
            start = furthest + 1;
        } else {
            start += 1;
        }
    }

    // A side straddling index 0 shows up as two runs; rejoin them.
    if runs.len() >= 2 {
        let first = runs[0];
        let last = runs[runs.len() - 1];
        if first.0 == 0 && last.1 == count - 1 {
            let mut moments = Moments::default();
            for &point in &ring[last.0..] {
                moments.add(point);
            }
            for &point in &ring[..=first.1] {
                moments.add(point);
            }
            if moments.rms() <= rms_limit {
                runs.remove(0);
                runs.pop();
                runs.push((last.0, first.1));
            }
        }
    }

    runs
}

/// Fit a contour that has been divided at `breaks`.
fn fit_segmented(ring: &[Point], breaks: &[Break], tolerance: f64) -> Contour {
    let span_count = breaks.len();

    struct Span {
        points: Vec<Point>,
        line: Option<(Point, Point)>,
    }

    let mut spans: Vec<Span> = Vec::with_capacity(span_count);
    for index in 0..span_count {
        let start = breaks[index].index;
        let end = breaks[(index + 1) % span_count].index;
        let points = slice_ring(ring, start, end);
        let line = fit_straight_run(&points, tolerance);
        spans.push(Span { points, line });
    }

    // Resolve every break position before emitting geometry: a corner between
    // two straight sides is defined by their intersection, not by a traced
    // vertex that the tracer chamfered.
    let mut vertices: Vec<Point> = Vec::with_capacity(span_count);
    for index in 0..span_count {
        let traced = ring[breaks[index].index];
        let incoming = spans[(index + span_count - 1) % span_count].line;
        let outgoing = spans[index].line;

        let resolved = match (incoming, outgoing) {
            (Some((a_point, a_direction)), Some((b_point, b_direction)))
                if breaks[index].sharp =>
            {
                line_intersection(a_point, a_direction, b_point, b_direction)
                    .filter(|candidate| candidate.distance(traced) < 4.0 + tolerance * 4.0)
                    .unwrap_or(traced)
            }
            (Some((point, direction)), _) | (_, Some((point, direction))) => {
                project_onto_line(traced, point, direction)
            }
            (None, None) => traced,
        };
        vertices.push(resolved);
    }

    let mut segments = Vec::new();
    for index in 0..span_count {
        let start_vertex = vertices[index];
        let end_vertex = vertices[(index + 1) % span_count];

        if spans[index].line.is_some() {
            segments.push(Segment::Line { to: end_vertex });
            continue;
        }

        let mut points = spans[index].points.clone();
        if points.len() < 2 {
            segments.push(Segment::Line { to: end_vertex });
            continue;
        }
        let last = points.len() - 1;
        points[0] = start_vertex;
        points[last] = end_vertex;

        // At a smooth junction against a straight side, take the tangent from
        // that side so the curve leaves it without a kink. At a corner, take it
        // one-sided so the corner stays sharp.
        let previous = (index + span_count - 1) % span_count;
        let next = (index + 1) % span_count;

        let start_tangent = if breaks[index].sharp {
            one_sided_tangent(&points, true)
        } else {
            spans[previous]
                .line
                .map(|(_, direction)| {
                    orient(direction, start_vertex.sub(vertices[previous]))
                })
                .unwrap_or_else(|| one_sided_tangent(&points, true))
        };

        let end_tangent = if breaks[next].sharp {
            one_sided_tangent(&points, false)
        } else {
            spans[next]
                .line
                .map(|(_, direction)| {
                    orient(direction, vertices[(next + 1) % span_count].sub(end_vertex))
                        .scale(-1.0)
                })
                .unwrap_or_else(|| one_sided_tangent(&points, false))
        };

        fit_cubic(&points, start_tangent, end_tangent, tolerance, &mut segments);
    }

    Contour {
        start: vertices[0],
        segments,
    }
}

/// Flip `direction` so it points the same way as `along`.
///
/// A total-least-squares line fit has no inherent orientation, so the sign has
/// to be recovered from the direction of travel.
fn orient(direction: Point, along: Point) -> Point {
    if direction.dot(along) >= 0.0 {
        direction
    } else {
        direction.scale(-1.0)
    }
}

/// Collapse geometry that is straighter than the error budget.
///
/// The curve fitter emits a cubic for every run it fitted, including runs that
/// turned out flat, and it may split one long straight side across several
/// segments. Both are wasted nodes, and both make the result harder to edit —
/// a designer wants the side of a rounded rectangle to be one line, not three
/// cubics with collinear handles.
pub fn straighten(contour: Contour, tolerance: f64) -> Contour {
    let count = contour.segments.len();
    if count < 3 {
        return contour;
    }

    // Cyclic vertex list: `vertices[i]` is where `kinds[i]` starts.
    let mut vertices: Vec<Point> = Vec::with_capacity(count);
    let mut kinds: Vec<Segment> = Vec::with_capacity(count);
    let mut current = contour.start;
    for segment in &contour.segments {
        vertices.push(current);
        // Replace a cubic that never leaves its own chord with that chord.
        let flattened = match *segment {
            Segment::Cubic { c1, c2, to } => {
                if cubic_chord_deviation(current, c1, c2, to) <= tolerance {
                    Segment::Line { to }
                } else {
                    *segment
                }
            }
            line => line,
        };
        kinds.push(flattened);
        current = segment.end_point();
    }

    // A joint can be dissolved when both sides are lines and the vertex sits
    // within tolerance of the merged chord.
    let removable = |vertices: &[Point], kinds: &[Segment], index: usize| -> bool {
        let n = vertices.len();
        if n < 4 {
            return false;
        }
        let previous = (index + n - 1) % n;
        if !matches!(kinds[previous], Segment::Line { .. })
            || !matches!(kinds[index], Segment::Line { .. })
        {
            return false;
        }
        let a = vertices[previous];
        let b = vertices[index];
        let c = vertices[(index + 1) % n];
        crate::geom::distance_to_line(b, a, c) <= tolerance
    };

    // Rotate so the start sits on a joint that will survive, otherwise merging
    // could never dissolve the wrap-around joint and the start point would be
    // stranded in the middle of a straight side.
    if let Some(anchor) = (0..vertices.len()).find(|&index| !removable(&vertices, &kinds, index)) {
        vertices.rotate_left(anchor);
        kinds.rotate_left(anchor);
    }

    // Dissolve removable joints. Walk forward, never touching index 0 so the
    // start point stays put.
    let mut index = 1;
    while index < vertices.len() && vertices.len() > 3 {
        if removable(&vertices, &kinds, index) {
            // The previous line now runs all the way to the next vertex.
            let next = vertices[(index + 1) % vertices.len()];
            let previous = (index + vertices.len() - 1) % vertices.len();
            kinds[previous] = Segment::Line { to: next };
            vertices.remove(index);
            kinds.remove(index);
            // Re-test the same position, since the joint before it may now
            // also be removable.
            index = index.saturating_sub(1).max(1);
        } else {
            index += 1;
        }
    }

    // Rebuild, re-anchoring each segment's endpoint to the surviving vertices.
    let total = vertices.len();
    let segments = (0..total)
        .map(|slot| {
            let end = vertices[(slot + 1) % total];
            match kinds[slot] {
                Segment::Line { .. } => Segment::Line { to: end },
                Segment::Cubic { c1, c2, .. } => Segment::Cubic { c1, c2, to: end },
            }
        })
        .collect();

    Contour {
        start: vertices[0],
        segments,
    }
}

/// Upper bound on how far a cubic strays from the straight chord between its
/// endpoints. The true maximum is at most three quarters of the largest
/// perpendicular control-point offset.
fn cubic_chord_deviation(p0: Point, c1: Point, c2: Point, p3: Point) -> f64 {
    let first = crate::geom::distance_to_line(c1, p0, p3);
    let second = crate::geom::distance_to_line(c2, p0, p3);
    0.75 * first.max(second)
}

/// Fit a closed contour whose break points are all corners.
///
/// Straight sides are recovered as exact lines meeting at exact corners;
/// everything else is fitted with cubics whose end tangents are taken
/// one-sided, so corners stay sharp.
///
/// The returned contour's start point is the *resolved* first corner, which is
/// generally not the traced vertex: recovering a sharp corner moves it.
pub fn fit_with_corners(ring: &[Point], corners: &[usize], tolerance: f64) -> Contour {
    if corners.len() < 2 || ring.len() < 4 {
        return Contour {
            start: ring[0],
            segments: fit_closed_smooth(ring, tolerance),
        };
    }

    let breaks: Vec<Break> = corners
        .iter()
        .map(|&index| Break { index, sharp: true })
        .collect();
    fit_segmented(ring, &breaks, tolerance)
}

/// Points from `start` to `end` inclusive, wrapping around the ring.
fn slice_ring(ring: &[Point], start: usize, end: usize) -> Vec<Point> {
    let count = ring.len();
    let mut points = Vec::new();
    let mut index = start;
    loop {
        points.push(ring[index]);
        if index == end {
            break;
        }
        index = (index + 1) % count;
        if points.len() > count {
            break;
        }
    }
    points
}

/// If the span is straight, return a point on its line and the unit direction.
///
/// The fit deliberately ignores points near both ends: those are where the
/// tracer's corner chamfer lives, and including them tilts the line — which is
/// the whole reason a chamfered square cannot be recognised as one without this.
fn fit_straight_run(points: &[Point], tolerance: f64) -> Option<(Point, Point)> {
    let count = points.len();
    if count < 2 {
        return None;
    }
    if count == 2 {
        // Two samples describe a line and nothing else.
        let direction = points[1].sub(points[0]).normalized();
        return (direction.length_sq() > 1e-18).then_some((points[0], direction));
    }

    let core = span_core(points, CORNER_TRIM);
    let (centroid, direction, residual) = fit_line(core)?;
    if residual > tolerance.max(TRACER_NOISE) {
        return None;
    }

    // A straight run also has to actually be traversed in one direction; a
    // hairpin can produce a low residual while doubling back on itself.
    let span = core[core.len() - 1].sub(core[0]);
    if span.dot(direction).abs() < span.length() * 0.99 {
        return None;
    }

    Some((centroid, direction))
}

/// The interior of a span, with `trim` of arc length removed from each end.
///
/// Always keeps at least three points. A two-point fit has zero residual by
/// construction and would declare anything straight; and when a span is too
/// short to give up `trim` at both ends, returning the untrimmed points would
/// hand back exactly the chamfered corner samples this is meant to exclude. The
/// middle three are used instead — a span that short is well described by a line
/// regardless, since its own sagitta is a small fraction of a pixel.
fn span_core(points: &[Point], trim: f64) -> &[Point] {
    let count = points.len();
    if count <= 3 {
        return points;
    }

    let mut first = 0usize;
    let mut travelled = 0.0;
    while first + 1 < count && travelled < trim {
        travelled += points[first].distance(points[first + 1]);
        first += 1;
    }

    let mut last = count - 1;
    travelled = 0.0;
    while last > 0 && travelled < trim {
        travelled += points[last].distance(points[last - 1]);
        last -= 1;
    }

    if last >= first + 2 {
        return &points[first..=last];
    }

    let middle = count / 2;
    let start = middle.saturating_sub(1).min(count - 3);
    &points[start..start + 3]
}

fn project_onto_line(point: Point, on_line: Point, direction: Point) -> Point {
    let offset = point.sub(on_line);
    on_line.add(direction.scale(offset.dot(direction)))
}

/// Tangent at one end of an open point run, estimated from that side only so a
/// corner is never smoothed across.
fn one_sided_tangent(points: &[Point], at_start: bool) -> Point {
    let count = points.len();
    if count < 2 {
        return Point::new(1.0, 0.0);
    }

    // Average over a short run for stability against tracer noise, but stay
    // local enough that the estimate still belongs to this end.
    let span = 3.min(count - 1);
    let tangent = if at_start {
        let mut accumulated = Point::new(0.0, 0.0);
        for index in 1..=span {
            let weight = 1.0 / index as f64;
            accumulated = accumulated.add(points[index].sub(points[0]).normalized().scale(weight));
        }
        accumulated
    } else {
        let last = count - 1;
        let mut accumulated = Point::new(0.0, 0.0);
        for index in 1..=span {
            let weight = 1.0 / index as f64;
            accumulated = accumulated.add(
                points[last - index]
                    .sub(points[last])
                    .normalized()
                    .scale(weight),
            );
        }
        accumulated
    };

    let normalized = tangent.normalized();
    if normalized.length_sq() < 1e-18 {
        if at_start {
            points[1].sub(points[0]).normalized()
        } else {
            points[count - 2].sub(points[count - 1]).normalized()
        }
    } else {
        normalized
    }
}

/// Schneider's recursive cubic fit. Appends segments reaching `points.last()`.
fn fit_cubic(
    points: &[Point],
    start_tangent: Point,
    end_tangent: Point,
    tolerance: f64,
    output: &mut Vec<Segment>,
) {
    let count = points.len();
    if count < 2 {
        return;
    }

    if count == 2 {
        let distance = points[0].distance(points[1]) / 3.0;
        output.push(Segment::Cubic {
            c1: points[0].add(start_tangent.scale(distance)),
            c2: points[1].add(end_tangent.scale(distance)),
            to: points[1],
        });
        return;
    }

    let mut parameters = chord_length_parameterize(points);
    let mut curve = generate_bezier(points, &parameters, start_tangent, end_tangent);
    let (mut error, mut split_at) = max_error(points, &parameters, &curve);

    if error < tolerance {
        push_curve(output, &curve);
        return;
    }

    // Close enough to be worth polishing rather than splitting.
    if error < tolerance * 4.0 {
        for _ in 0..MAX_NEWTON_ITERATIONS {
            parameters = reparameterize(points, &parameters, &curve);
            curve = generate_bezier(points, &parameters, start_tangent, end_tangent);
            let (new_error, new_split) = max_error(points, &parameters, &curve);
            error = new_error;
            split_at = new_split;
            if error < tolerance {
                push_curve(output, &curve);
                return;
            }
        }
    }

    // Split at the worst point and recurse. Guard against a degenerate split
    // that would recurse forever.
    let split_at = split_at.clamp(1, count - 2);
    let centre_tangent = centre_tangent(points, split_at);

    fit_cubic(
        &points[..=split_at],
        start_tangent,
        centre_tangent,
        tolerance,
        output,
    );
    fit_cubic(
        &points[split_at..],
        centre_tangent.scale(-1.0),
        end_tangent,
        tolerance,
        output,
    );
}

fn push_curve(output: &mut Vec<Segment>, curve: &[Point; 4]) {
    output.push(Segment::Cubic {
        c1: curve[1],
        c2: curve[2],
        to: curve[3],
    });
}

fn centre_tangent(points: &[Point], index: usize) -> Point {
    let before = points[index - 1].sub(points[index]);
    let after = points[index].sub(points[index + 1]);
    let tangent = Point::new(
        (before.x + after.x) * 0.5,
        (before.y + after.y) * 0.5,
    )
    .normalized();
    if tangent.length_sq() < 1e-18 {
        points[index].sub(points[index - 1]).normalized()
    } else {
        tangent
    }
}

fn chord_length_parameterize(points: &[Point]) -> Vec<f64> {
    let mut parameters = Vec::with_capacity(points.len());
    parameters.push(0.0);
    for index in 1..points.len() {
        let previous = parameters[index - 1];
        parameters.push(previous + points[index].distance(points[index - 1]));
    }

    let total = parameters[points.len() - 1];
    if total <= 0.0 {
        // Degenerate run; spread parameters evenly so the solve stays defined.
        let last = points.len() - 1;
        for (index, parameter) in parameters.iter_mut().enumerate() {
            *parameter = index as f64 / last as f64;
        }
        return parameters;
    }

    for parameter in parameters.iter_mut() {
        *parameter /= total;
    }
    parameters
}

/// Least-squares cubic through fixed endpoints with fixed end tangent
/// directions; solves for the two control-point distances.
fn generate_bezier(
    points: &[Point],
    parameters: &[f64],
    start_tangent: Point,
    end_tangent: Point,
) -> [Point; 4] {
    let first = points[0];
    let last = points[points.len() - 1];

    let mut c = [[0.0f64; 2]; 2];
    let mut x = [0.0f64; 2];

    for (index, &point) in points.iter().enumerate() {
        let t = parameters[index];
        let mt = 1.0 - t;
        let b0 = mt * mt * mt;
        let b1 = 3.0 * mt * mt * t;
        let b2 = 3.0 * mt * t * t;
        let b3 = t * t * t;

        let a0 = start_tangent.scale(b1);
        let a1 = end_tangent.scale(b2);

        c[0][0] += a0.dot(a0);
        c[0][1] += a0.dot(a1);
        c[1][0] = c[0][1];
        c[1][1] += a1.dot(a1);

        let target = point.sub(first.scale(b0 + b1).add(last.scale(b2 + b3)));
        x[0] += a0.dot(target);
        x[1] += a1.dot(target);
    }

    let det_c = c[0][0] * c[1][1] - c[1][0] * c[0][1];
    let det_x_c1 = x[0] * c[1][1] - c[0][1] * x[1];
    let det_c0_x = c[0][0] * x[1] - c[1][0] * x[0];

    let (mut alpha_start, mut alpha_end) = if det_c.abs() > 1e-12 {
        (det_x_c1 / det_c, det_c0_x / det_c)
    } else {
        (0.0, 0.0)
    };

    // Wu/Barsky fallback when the solve is degenerate or pushes control points
    // backwards through the endpoints.
    let chord = first.distance(last);
    let minimum = chord * 1e-6;
    if alpha_start < minimum || alpha_end < minimum {
        alpha_start = chord / 3.0;
        alpha_end = chord / 3.0;
    }

    [
        first,
        first.add(start_tangent.scale(alpha_start)),
        last.add(end_tangent.scale(alpha_end)),
        last,
    ]
}

fn max_error(points: &[Point], parameters: &[f64], curve: &[Point; 4]) -> (f64, usize) {
    let mut worst = 0.0;
    let mut worst_index = points.len() / 2;
    for index in 1..points.len() - 1 {
        let on_curve = eval_cubic(curve[0], curve[1], curve[2], curve[3], parameters[index]);
        let distance = on_curve.distance_sq(points[index]);
        if distance > worst {
            worst = distance;
            worst_index = index;
        }
    }
    (worst.sqrt(), worst_index)
}

/// One Newton-Raphson step per point, moving each parameter to the value that
/// puts it closest to the curve.
fn reparameterize(points: &[Point], parameters: &[f64], curve: &[Point; 4]) -> Vec<f64> {
    points
        .iter()
        .zip(parameters.iter())
        .map(|(&point, &t)| newton_step(point, t, curve))
        .collect()
}

fn newton_step(point: Point, t: f64, curve: &[Point; 4]) -> f64 {
    let on_curve = eval_cubic(curve[0], curve[1], curve[2], curve[3], t);
    let first_derivative = eval_cubic_derivative(curve[0], curve[1], curve[2], curve[3], t);

    // Second derivative of a cubic Bezier.
    let mt = 1.0 - t;
    let second_derivative = curve[2]
        .sub(curve[1].scale(2.0))
        .add(curve[0])
        .scale(6.0 * mt)
        .add(
            curve[3]
                .sub(curve[2].scale(2.0))
                .add(curve[1])
                .scale(6.0 * t),
        );

    let difference = on_curve.sub(point);
    let numerator = difference.dot(first_derivative);
    let denominator = first_derivative.dot(first_derivative) + difference.dot(second_derivative);

    if denominator.abs() < 1e-12 {
        t
    } else {
        (t - numerator / denominator).clamp(0.0, 1.0)
    }
}

fn polygon_segments(ring: &[Point]) -> Vec<Segment> {
    ring.iter()
        .skip(1)
        .map(|&to| Segment::Line { to })
        .chain(ring.first().map(|&to| Segment::Line { to }))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::path::Contour;

    fn sample_circle(radius: f64, samples: usize) -> Vec<Point> {
        (0..samples)
            .map(|index| {
                let angle = index as f64 / samples as f64 * std::f64::consts::TAU;
                Point::new(radius * angle.cos(), radius * angle.sin())
            })
            .collect()
    }

    /// Distance from `point` to the segment `a`-`b`.
    fn distance_to_segment(point: Point, a: Point, b: Point) -> f64 {
        let span = b.sub(a);
        let length_sq = span.length_sq();
        if length_sq < 1e-18 {
            return point.distance(a);
        }
        let t = (point.sub(a).dot(span) / length_sq).clamp(0.0, 1.0);
        point.distance(a.add(span.scale(t)))
    }

    /// Worst distance from any reference point to the fitted curve.
    ///
    /// Measured against the flattened *polyline*, not its vertices: sampling
    /// spacing would otherwise be mistaken for fitting error.
    fn worst_deviation(contour: &Contour, reference: &[Point]) -> f64 {
        let flattened = contour.flatten(0.005);
        let count = flattened.len();
        assert!(count >= 3, "flattened contour was degenerate");

        let mut worst: f64 = 0.0;
        for &point in reference {
            let mut nearest = f64::INFINITY;
            for index in 0..count {
                let a = flattened[index];
                let b = flattened[(index + 1) % count];
                nearest = nearest.min(distance_to_segment(point, a, b));
            }
            worst = worst.max(nearest);
        }
        worst
    }

    #[test]
    fn a_smooth_circle_fits_with_very_few_segments() {
        let ring = sample_circle(30.0, 200);
        let segments = fit_closed_smooth(&ring, 0.2);
        assert!(
            segments.len() <= 8,
            "expected a handful of cubics, got {}",
            segments.len()
        );

        let contour = Contour {
            start: ring[0],
            segments,
        };
        let deviation = worst_deviation(&contour, &ring);
        assert!(deviation < 0.2, "deviation was {deviation:.4}px");
    }

    #[test]
    fn a_chamfered_square_recovers_exact_corners() {
        // Build a square whose corners have been cut off by half a pixel, the
        // way marching squares leaves them, and check the fit puts them back.
        let side = 40.0;
        let chamfer = 0.5;
        let mut ring = Vec::new();
        let corners = [
            Point::new(0.0, 0.0),
            Point::new(side, 0.0),
            Point::new(side, side),
            Point::new(0.0, side),
        ];
        for index in 0..4 {
            let current = corners[index];
            let previous = corners[(index + 3) % 4];
            let next = corners[(index + 1) % 4];
            let entry = current.lerp(previous, chamfer / current.distance(previous));
            let exit = current.lerp(next, chamfer / current.distance(next));
            ring.push(entry);
            ring.push(exit);
            // Dense samples along the straight run to the next corner.
            for step in 1..40 {
                ring.push(exit.lerp(next, step as f64 / 40.0));
            }
        }

        let corner_indices = crate::corner::detect_corners(&ring, &Default::default());
        assert_eq!(corner_indices.len(), 4, "got {corner_indices:?}");

        let contour = fit_with_corners(&ring, &corner_indices, 0.3);

        // Every segment should be a straight line, and the area should match
        // the true square rather than the chamfered one.
        assert!(
            contour
                .segments
                .iter()
                .all(|segment| matches!(segment, Segment::Line { .. })),
            "expected only lines, got {:?}",
            contour.segments
        );
        let area = contour.area().abs();
        assert!(
            (area - side * side).abs() < 1.0,
            "area {area} should be close to {}",
            side * side
        );
    }

    #[test]
    fn a_straight_line_needs_exactly_one_segment() {
        let points: Vec<Point> = (0..50)
            .map(|index| Point::new(index as f64, index as f64 * 0.5))
            .collect();
        let mut segments = Vec::new();
        let tangent = Point::new(1.0, 0.5).normalized();
        fit_cubic(&points, tangent, tangent.scale(-1.0), 0.2, &mut segments);
        assert_eq!(segments.len(), 1);
    }

    #[test]
    fn tighter_tolerance_never_increases_error() {
        let ring = sample_circle(25.0, 180);
        let loose = Contour {
            start: ring[0],
            segments: fit_closed_smooth(&ring, 0.5),
        };
        let tight = Contour {
            start: ring[0],
            segments: fit_closed_smooth(&ring, 0.05),
        };
        assert!(worst_deviation(&tight, &ring) <= worst_deviation(&loose, &ring) + 1e-9);
        assert!(tight.segments.len() >= loose.segments.len());
    }

    #[test]
    fn fitting_is_deterministic() {
        let ring = sample_circle(18.0, 120);
        let first = fit_closed_smooth(&ring, 0.25);
        for _ in 0..8 {
            assert_eq!(first, fit_closed_smooth(&ring, 0.25));
        }
    }
}
