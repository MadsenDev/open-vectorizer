//! Contour extraction.
//!
//! Two tracers live here:
//!
//! * [`trace_level`] runs marching squares over a coverage field and places
//!   every vertex by linear interpolation, so an anti-aliased boundary is
//!   recovered to a fraction of a pixel rather than snapped to the pixel grid.
//! * [`trace_cells`] walks the exact edges between filled and empty cells,
//!   which is what pixel art wants: no interpolation, no chamfering.

use std::collections::HashMap;

use crate::field::Field;
use crate::geom::{signed_area, Point};

/// Marching-squares contour extraction at `level`.
///
/// The field is padded with a ring of zeros positioned one sample outside the
/// image, so a region that runs off the edge of the canvas closes exactly along
/// the canvas border instead of being left open.
pub fn trace_level(field: &Field, level: f32) -> Vec<Vec<Point>> {
    let grid_width = field.width + 2;
    let grid_height = field.height + 2;
    if grid_width < 2 || grid_height < 2 {
        return Vec::new();
    }

    let sample = |i: usize, j: usize| -> f32 {
        if i == 0 || j == 0 || i == grid_width - 1 || j == grid_height - 1 {
            0.0
        } else {
            field.get(i - 1, j - 1)
        }
    };

    let horizontal_count = (grid_width - 1) * grid_height;
    let horizontal_id = |i: usize, j: usize| -> u32 { (j * (grid_width - 1) + i) as u32 };
    let vertical_id =
        |i: usize, j: usize| -> u32 { (horizontal_count + j * grid_width + i) as u32 };

    // Each grid edge carries at most one crossing, and every crossing is the
    // start of exactly one segment and the end of exactly one other. That makes
    // the link map a permutation, so chaining it always yields closed loops.
    let mut next: HashMap<u32, u32> = HashMap::new();
    let mut starts: Vec<u32> = Vec::new();

    for j in 0..grid_height - 1 {
        for i in 0..grid_width - 1 {
            let v00 = sample(i, j);
            let v10 = sample(i + 1, j);
            let v11 = sample(i + 1, j + 1);
            let v01 = sample(i, j + 1);

            let mut case = 0u8;
            if v00 >= level {
                case |= 1;
            }
            if v10 >= level {
                case |= 2;
            }
            if v11 >= level {
                case |= 4;
            }
            if v01 >= level {
                case |= 8;
            }

            if case == 0 || case == 15 {
                continue;
            }

            let top = horizontal_id(i, j);
            let bottom = horizontal_id(i, j + 1);
            let left = vertical_id(i, j);
            let right = vertical_id(i + 1, j);

            let mut connect = |from: u32, to: u32| {
                next.insert(from, to);
                starts.push(from);
            };

            match case {
                1 => connect(left, top),
                2 => connect(top, right),
                3 => connect(left, right),
                4 => connect(right, bottom),
                6 => connect(top, bottom),
                7 => connect(left, bottom),
                8 => connect(bottom, left),
                9 => connect(bottom, top),
                11 => connect(bottom, right),
                12 => connect(right, left),
                13 => connect(right, top),
                14 => connect(top, left),
                5 | 10 => {
                    // Saddle. The cell centre decides whether the two "inside"
                    // corners are joined through the middle or separated.
                    let centre = (v00 + v10 + v11 + v01) * 0.25;
                    let joined = centre >= level;
                    if case == 5 {
                        if joined {
                            connect(right, top);
                            connect(left, bottom);
                        } else {
                            connect(left, top);
                            connect(right, bottom);
                        }
                    } else if joined {
                        connect(top, left);
                        connect(bottom, right);
                    } else {
                        connect(top, right);
                        connect(bottom, left);
                    }
                }
                _ => {}
            }
        }
    }

    // Decode an edge id back into the interpolated crossing position. Both
    // cells sharing an edge run identical arithmetic, so the positions agree
    // exactly and the chain never develops gaps.
    let position = |edge: u32| -> Point {
        let edge = edge as usize;
        if edge < horizontal_count {
            let j = edge / (grid_width - 1);
            let i = edge % (grid_width - 1);
            let a = sample(i, j);
            let b = sample(i + 1, j);
            let t = crossing(a, b, level);
            Point::new(i as f64 - 0.5 + t, j as f64 - 0.5)
        } else {
            let edge = edge - horizontal_count;
            let j = edge / grid_width;
            let i = edge % grid_width;
            let a = sample(i, j);
            let b = sample(i, j + 1);
            let t = crossing(a, b, level);
            Point::new(i as f64 - 0.5, j as f64 - 0.5 + t)
        }
    };

    let mut contours = Vec::new();
    let mut visited: HashMap<u32, bool> = HashMap::with_capacity(next.len());

    for &start in &starts {
        if visited.contains_key(&start) {
            continue;
        }

        let mut points = Vec::new();
        let mut current = start;
        loop {
            if visited.insert(current, true).is_some() {
                break;
            }
            points.push(position(current));
            match next.get(&current) {
                Some(&following) => current = following,
                None => break,
            }
            if current == start {
                break;
            }
        }

        if points.len() >= 3 {
            contours.push(points);
        }
    }

    contours
}

fn crossing(a: f32, b: f32, level: f32) -> f64 {
    let denominator = b - a;
    if denominator.abs() < 1e-9 {
        0.5
    } else {
        (((level - a) / denominator) as f64).clamp(0.0, 1.0)
    }
}

/// Trace the exact cell edges of a binary mask.
///
/// Produces axis-aligned contours on integer coordinates: the true raster
/// silhouette, with no interpolation and no corner chamfering.
pub fn trace_cells(mask: &[bool], width: usize, height: usize) -> Vec<Vec<Point>> {
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let filled = |x: i64, y: i64| -> bool {
        if x < 0 || y < 0 || x >= width as i64 || y >= height as i64 {
            false
        } else {
            mask[y as usize * width + x as usize]
        }
    };

    let vertex_stride = width + 1;
    let vertex_id = |x: usize, y: usize| -> u32 { (y * vertex_stride + x) as u32 };

    // Directed edges, emitted so the filled side is always on the right.
    let mut edges: Vec<(u32, u32)> = Vec::new();
    for y in 0..height {
        for x in 0..width {
            if !filled(x as i64, y as i64) {
                continue;
            }
            let (xi, yi) = (x as i64, y as i64);
            if !filled(xi, yi - 1) {
                edges.push((vertex_id(x, y), vertex_id(x + 1, y)));
            }
            if !filled(xi + 1, yi) {
                edges.push((vertex_id(x + 1, y), vertex_id(x + 1, y + 1)));
            }
            if !filled(xi, yi + 1) {
                edges.push((vertex_id(x + 1, y + 1), vertex_id(x, y + 1)));
            }
            if !filled(xi - 1, yi) {
                edges.push((vertex_id(x, y + 1), vertex_id(x, y)));
            }
        }
    }

    if edges.is_empty() {
        return Vec::new();
    }

    let mut outgoing: HashMap<u32, Vec<usize>> = HashMap::new();
    for (index, &(from, _)) in edges.iter().enumerate() {
        outgoing.entry(from).or_default().push(index);
    }

    let decode = |vertex: u32| -> Point {
        let vertex = vertex as usize;
        Point::new(
            (vertex % vertex_stride) as f64,
            (vertex / vertex_stride) as f64,
        )
    };

    let mut used = vec![false; edges.len()];
    let mut contours = Vec::new();

    // Iterate in raster order so the output does not depend on hash ordering.
    for seed in 0..edges.len() {
        if used[seed] {
            continue;
        }

        let mut points = Vec::new();
        let mut edge = seed;
        let start_vertex = edges[seed].0;

        loop {
            used[edge] = true;
            let (from, to) = edges[edge];
            points.push(decode(from));

            if to == start_vertex {
                break;
            }

            let Some(candidates) = outgoing.get(&to) else {
                break;
            };

            let incoming = decode(to).sub(decode(from));
            let mut best: Option<(usize, f64)> = None;
            for &candidate in candidates {
                if used[candidate] {
                    continue;
                }
                let direction = decode(edges[candidate].1).sub(decode(edges[candidate].0));
                // Prefer the most counter-clockwise continuation, which keeps
                // diagonally touching cells on a single 8-connected outline.
                let score = turn_score(incoming, direction);
                if best.is_none_or(|(_, best_score)| score > best_score) {
                    best = Some((candidate, score));
                }
            }

            match best {
                Some((candidate, _)) => edge = candidate,
                None => break,
            }
        }

        if points.len() >= 4 {
            points.push(points[0]);
            points.pop();
            contours.push(points);
        }
    }

    contours
}

/// Ranks a turn from `incoming` to `outgoing`, largest meaning "most
/// counter-clockwise" in the y-down image frame.
fn turn_score(incoming: Point, outgoing: Point) -> f64 {
    let cross = incoming.cross(outgoing);
    let dot = incoming.dot(outgoing);
    // atan2 gives a single continuous ordering over the full turn circle.
    -cross.atan2(dot)
}

/// Nesting relationship between traced contours.
pub struct Nesting {
    /// Indices of contours at even nesting depth: the filled outlines.
    pub outers: Vec<usize>,
    /// For each outer, the indices of the contours that cut holes in it.
    pub holes: Vec<Vec<usize>>,
}

/// Work out which contours are outlines and which are holes.
///
/// Depth is the number of contours strictly containing a given contour, so this
/// handles arbitrary nesting: a dot inside the hole of a ring comes back as a
/// filled outline again.
pub fn nest(contours: &[Vec<Point>]) -> Nesting {
    let count = contours.len();
    let bounds: Vec<_> = contours
        .iter()
        .map(|contour| crate::geom::Bounds::of_points(contour))
        .collect();

    let mut depth = vec![0usize; count];
    let mut parent = vec![None; count];

    for i in 0..count {
        // A vertex sits on the boundary, so probe with an interior point.
        let probe = interior_probe(&contours[i]);
        let mut best_parent: Option<(usize, f64)> = None;

        for j in 0..count {
            if i == j || !bounds[j].contains_bounds(&bounds[i]) {
                continue;
            }
            if crate::geom::point_in_polygon(probe, &contours[j]) {
                depth[i] += 1;
                // The immediate parent is the smallest container.
                let area = signed_area(&contours[j]).abs();
                if best_parent.is_none_or(|(_, best_area)| area < best_area) {
                    best_parent = Some((j, area));
                }
            }
        }
        parent[i] = best_parent.map(|(index, _)| index);
    }

    let mut outers = Vec::new();
    let mut index_of_outer = vec![usize::MAX; count];
    for i in 0..count {
        if depth[i].is_multiple_of(2) {
            index_of_outer[i] = outers.len();
            outers.push(i);
        }
    }

    let mut holes = vec![Vec::new(); outers.len()];
    for i in 0..count {
        if depth[i] % 2 == 1 {
            if let Some(parent_index) = parent[i] {
                let slot = index_of_outer[parent_index];
                if slot != usize::MAX {
                    holes[slot].push(i);
                }
            }
        }
    }

    Nesting { outers, holes }
}

/// A point guaranteed to be strictly inside the polygon.
fn interior_probe(polygon: &[Point]) -> Point {
    // Try the centroid first; it is inside for any convex shape and most
    // others. Fall back to scanning a horizontal ray for a genuine interior
    // span when the shape is concave enough for the centroid to fall out.
    let count = polygon.len() as f64;
    let mut centroid = Point::new(0.0, 0.0);
    for &point in polygon {
        centroid = centroid.add(point);
    }
    centroid = centroid.scale(1.0 / count);
    if crate::geom::point_in_polygon(centroid, polygon) {
        return centroid;
    }

    // Scan along the y of an edge midpoint, collecting crossings, and take the
    // midpoint of the widest interior span.
    for window in polygon.windows(2) {
        let y = (window[0].y + window[1].y) * 0.5;
        let mut crossings: Vec<f64> = Vec::new();
        let mut previous = polygon[polygon.len() - 1];
        for &current in polygon {
            if (previous.y > y) != (current.y > y) {
                let denominator = current.y - previous.y;
                if denominator.abs() > 1e-12 {
                    crossings
                        .push(previous.x + (y - previous.y) / denominator * (current.x - previous.x));
                }
            }
            previous = current;
        }
        crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let mut best: Option<(f64, f64)> = None;
        for pair in crossings.chunks_exact(2) {
            let width = pair[1] - pair[0];
            if best.is_none_or(|(best_width, _)| width > best_width) {
                best = Some((width, (pair[0] + pair[1]) * 0.5));
            }
        }
        if let Some((width, x)) = best {
            if width > 1e-9 {
                return Point::new(x, y);
            }
        }
    }

    centroid
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::signed_area;

    fn disc_field(size: usize, radius: f64) -> Field {
        // Analytic coverage: how much of each pixel the disc covers, estimated
        // by supersampling, exactly as a real anti-aliased render would.
        let mut field = Field::new(size, size);
        let centre = size as f64 / 2.0;
        const SUB: usize = 8;
        for y in 0..size {
            for x in 0..size {
                let mut hits = 0;
                for sy in 0..SUB {
                    for sx in 0..SUB {
                        let px = x as f64 + (sx as f64 + 0.5) / SUB as f64;
                        let py = y as f64 + (sy as f64 + 0.5) / SUB as f64;
                        let dx = px - centre;
                        let dy = py - centre;
                        if dx * dx + dy * dy <= radius * radius {
                            hits += 1;
                        }
                    }
                }
                field.set(x, y, hits as f32 / (SUB * SUB) as f32);
            }
        }
        field
    }

    #[test]
    fn marching_squares_recovers_a_circle_to_sub_pixel_accuracy() {
        let size = 64;
        let radius = 20.0;
        let field = disc_field(size, radius);
        let contours = trace_level(&field, 0.5);

        assert_eq!(contours.len(), 1, "a disc has exactly one boundary");
        let centre = Point::new(size as f64 / 2.0, size as f64 / 2.0);

        let mut worst: f64 = 0.0;
        for &point in &contours[0] {
            worst = worst.max((point.distance(centre) - radius).abs());
        }
        // The old integer-grid tracer cannot do better than half a pixel here.
        assert!(
            worst < 0.1,
            "worst radial error was {worst:.4}px, expected well under 0.1"
        );
    }

    #[test]
    fn marching_squares_recovers_a_hard_edged_square_exactly() {
        let size = 32;
        let mut field = Field::new(size, size);
        for y in 8..24 {
            for x in 8..24 {
                field.set(x, y, 1.0);
            }
        }
        let contours = trace_level(&field, 0.5);
        assert_eq!(contours.len(), 1);

        // Corners get chamfered by marching squares; the straight runs should
        // still land exactly on the true edges at 8.0 and 24.0.
        let bounds = crate::geom::Bounds::of_points(&contours[0]);
        assert!((bounds.min_x - 8.0).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.max_x - 24.0).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.min_y - 8.0).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.max_y - 24.0).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn marching_squares_separates_a_ring_into_outer_and_inner() {
        let size = 48;
        let outer = disc_field(size, 18.0);
        let inner = disc_field(size, 9.0);
        let mut ring = Field::new(size, size);
        for index in 0..ring.data.len() {
            ring.data[index] = (outer.data[index] - inner.data[index]).max(0.0);
        }

        let contours = trace_level(&ring, 0.5);
        assert_eq!(contours.len(), 2, "a ring has an outer and an inner contour");

        let nesting = nest(&contours);
        assert_eq!(nesting.outers.len(), 1);
        assert_eq!(nesting.holes[0].len(), 1);
    }

    #[test]
    fn a_region_touching_the_canvas_edge_closes_on_the_border() {
        let mut field = Field::new(8, 8);
        for y in 0..8 {
            for x in 0..4 {
                field.set(x, y, 1.0);
            }
        }
        let contours = trace_level(&field, 0.5);
        assert_eq!(contours.len(), 1);
        let bounds = crate::geom::Bounds::of_points(&contours[0]);
        assert!((bounds.min_x - 0.0).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.min_y - 0.0).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.max_y - 8.0).abs() < 1e-9, "{bounds:?}");
        assert!((bounds.max_x - 4.0).abs() < 1e-9, "{bounds:?}");
    }

    #[test]
    fn cell_tracer_keeps_a_single_pixel_square() {
        let mask = vec![false, false, false, false, true, false, false, false, false];
        let contours = trace_cells(&mask, 3, 3);
        assert_eq!(contours.len(), 1);
        assert_eq!(signed_area(&contours[0]).abs(), 1.0);
    }

    #[test]
    fn cell_tracer_finds_holes() {
        // 3x3 ring with the centre knocked out.
        let mask = vec![true, true, true, true, false, true, true, true, true];
        let contours = trace_cells(&mask, 3, 3);
        assert_eq!(contours.len(), 2);
        let nesting = nest(&contours);
        assert_eq!(nesting.outers.len(), 1);
        assert_eq!(nesting.holes[0].len(), 1);
    }

    #[test]
    fn tracing_is_deterministic() {
        let field = disc_field(40, 12.0);
        let first = trace_level(&field, 0.5);
        for _ in 0..8 {
            assert_eq!(first, trace_level(&field, 0.5));
        }
    }
}
