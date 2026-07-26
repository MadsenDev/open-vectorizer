//! Anti-aliased scanline rasterizer.
//!
//! This exists so the engine can check its own work: candidate geometry is
//! rendered back to coverage and compared against the coverage measured from
//! the source image. Every accept/reject decision downstream — whether a region
//! really is a circle, whether a fit is tight enough, how many nodes a curve
//! needs — is settled by that measurement instead of by a guessed threshold.
//!
//! Coverage is exact in x (spans are clipped analytically against pixel
//! boundaries) and sampled at [`SUB_ROWS`] positions in y.

use crate::field::Field;
use crate::geom::Bounds;
use crate::path::Contour;

/// Sub-scanlines per pixel row. 16 keeps the vertical quantization error below
/// about 0.03 of a pixel's coverage, comfortably finer than the sub-pixel
/// accuracy the fitting stage is trying to verify.
pub const SUB_ROWS: usize = 16;

/// Flatness used when reducing cubics to line segments for rasterization.
const FLATTEN_TOLERANCE: f64 = 0.02;

/// A coverage buffer over an integer pixel window of the canvas.
#[derive(Debug, Clone)]
pub struct Mask {
    pub x0: i64,
    pub y0: i64,
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Mask {
    #[inline]
    pub fn get(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.width + x]
    }

    pub fn total(&self) -> f64 {
        self.data.iter().map(|&value| value as f64).sum()
    }
}

/// The integer pixel window covering `bounds`, clipped to the canvas.
pub fn window_for(bounds: Bounds, canvas_width: usize, canvas_height: usize) -> (i64, i64, usize, usize) {
    if bounds.is_empty() {
        return (0, 0, 0, 0);
    }
    let x0 = bounds.min_x.floor() as i64;
    let y0 = bounds.min_y.floor() as i64;
    let x1 = bounds.max_x.ceil() as i64;
    let y1 = bounds.max_y.ceil() as i64;

    let x0 = x0.clamp(0, canvas_width as i64);
    let y0 = y0.clamp(0, canvas_height as i64);
    let x1 = x1.clamp(0, canvas_width as i64);
    let y1 = y1.clamp(0, canvas_height as i64);

    (
        x0,
        y0,
        (x1 - x0).max(0) as usize,
        (y1 - y0).max(0) as usize,
    )
}

struct Edge {
    x_at_y0: f64,
    slope: f64,
    y_top: f64,
    y_bottom: f64,
}

/// Rasterize contours with the even-odd fill rule into the given window.
pub fn rasterize(contours: &[Contour], window: (i64, i64, usize, usize)) -> Mask {
    let (x0, y0, width, height) = window;
    let mut mask = Mask {
        x0,
        y0,
        width,
        height,
        data: vec![0.0; width * height],
    };
    if width == 0 || height == 0 {
        return mask;
    }

    let mut edges: Vec<Edge> = Vec::new();
    for contour in contours {
        let points = contour.flatten(FLATTEN_TOLERANCE);
        let count = points.len();
        if count < 3 {
            continue;
        }
        for index in 0..count {
            let a = points[index];
            let b = points[(index + 1) % count];
            if (a.y - b.y).abs() < 1e-12 {
                // Horizontal edges never cross a sub-scanline.
                continue;
            }
            let (top, bottom) = if a.y < b.y { (a, b) } else { (b, a) };
            let slope = (bottom.x - top.x) / (bottom.y - top.y);
            edges.push(Edge {
                x_at_y0: top.x,
                slope,
                y_top: top.y,
                y_bottom: bottom.y,
            });
        }
    }

    if edges.is_empty() {
        return mask;
    }

    // Bucket edges by the pixel rows they span so each row only visits the
    // edges that can possibly cross it.
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); height];
    for (index, edge) in edges.iter().enumerate() {
        let first = (edge.y_top.floor() as i64 - y0).clamp(0, height as i64);
        let last = (edge.y_bottom.ceil() as i64 - y0).clamp(0, height as i64);
        for row in first..last {
            buckets[row as usize].push(index);
        }
    }

    let sub_weight = 1.0 / SUB_ROWS as f32;
    let mut crossings: Vec<f64> = Vec::new();

    for (row, bucket) in buckets.iter().enumerate() {
        if bucket.is_empty() {
            continue;
        }
        let row_base = row * width;

        for sub in 0..SUB_ROWS {
            let sample_y = (y0 + row as i64) as f64 + (sub as f64 + 0.5) / SUB_ROWS as f64;

            crossings.clear();
            for &index in bucket {
                let edge = &edges[index];
                if sample_y < edge.y_top || sample_y >= edge.y_bottom {
                    continue;
                }
                crossings.push(edge.x_at_y0 + (sample_y - edge.y_top) * edge.slope);
            }
            if crossings.len() < 2 {
                continue;
            }
            crossings.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            // Even-odd: the interior lies between successive crossing pairs.
            for pair in crossings.chunks_exact(2) {
                let span_start = pair[0] - x0 as f64;
                let span_end = pair[1] - x0 as f64;
                if span_end <= 0.0 || span_start >= width as f64 {
                    continue;
                }
                let span_start = span_start.max(0.0);
                let span_end = span_end.min(width as f64);
                if span_end <= span_start {
                    continue;
                }

                let first_pixel = span_start.floor() as usize;
                let last_pixel = ((span_end.ceil() as usize).max(1) - 1).min(width - 1);

                for pixel in first_pixel..=last_pixel {
                    let left = (pixel as f64).max(span_start);
                    let right = ((pixel + 1) as f64).min(span_end);
                    if right > left {
                        mask.data[row_base + pixel] += (right - left) as f32 * sub_weight;
                    }
                }
            }
        }
    }

    for value in mask.data.iter_mut() {
        *value = value.clamp(0.0, 1.0);
    }

    mask
}

/// How well rendered geometry matches the coverage measured from the source.
#[derive(Debug, Clone, Copy)]
pub struct Comparison {
    /// Mean absolute coverage difference over the compared window.
    pub mean_error: f64,
    /// Largest single-pixel coverage difference.
    pub max_error: f64,
    /// Rendered area minus target area, in pixels.
    pub area_delta: f64,
}

/// Compare a rendered mask against the target coverage field.
///
/// The window is padded by one pixel when set up by the caller, so a shape that
/// renders slightly too large is still penalised rather than being clipped out
/// of the comparison.
pub fn compare(mask: &Mask, target: &Field) -> Comparison {
    compare_within(mask, target, None)
}

/// Compare only inside `scope`, a per-pixel flag over the mask's window.
///
/// A color's coverage field describes *every* region painted in that color, but
/// a single shape only accounts for its own. Comparing against the unrestricted
/// field lets a neighbouring region — or a speck the engine deliberately dropped
/// as noise — register as a whole pixel of unexplained coverage, which pins every
/// candidate's error at the maximum and destroys the engine's ability to tell
/// them apart.
pub fn compare_within(mask: &Mask, target: &Field, scope: Option<&[bool]>) -> Comparison {
    if mask.width == 0 || mask.height == 0 {
        return Comparison {
            mean_error: 0.0,
            max_error: 0.0,
            area_delta: 0.0,
        };
    }

    let mut sum_absolute = 0.0f64;
    let mut max_error = 0.0f64;
    let mut rendered_area = 0.0f64;
    let mut target_area = 0.0f64;
    let mut counted = 0usize;

    for row in 0..mask.height {
        let target_y = mask.y0 + row as i64;
        for column in 0..mask.width {
            let index = row * mask.width + column;
            if let Some(scope) = scope {
                if !scope.get(index).copied().unwrap_or(true) {
                    continue;
                }
            }

            let target_x = mask.x0 + column as i64;
            let expected = if target_x >= 0
                && target_y >= 0
                && (target_x as usize) < target.width
                && (target_y as usize) < target.height
            {
                target.get(target_x as usize, target_y as usize) as f64
            } else {
                0.0
            };

            let actual = mask.data[index] as f64;
            let difference = (actual - expected).abs();
            sum_absolute += difference;
            if difference > max_error {
                max_error = difference;
            }
            rendered_area += actual;
            target_area += expected;
            counted += 1;
        }
    }

    Comparison {
        mean_error: sum_absolute / counted.max(1) as f64,
        max_error,
        area_delta: rendered_area - target_area,
    }
}

/// Flags the pixels a shape is answerable for: everything its traced outline
/// covers, widened by `dilation` so the anti-aliased boundary band is included.
pub fn scope_of(
    contours: &[Contour],
    window: (i64, i64, usize, usize),
    dilation: usize,
) -> Vec<bool> {
    let (_, _, width, height) = window;
    if width == 0 || height == 0 {
        return Vec::new();
    }

    let covered = rasterize(contours, window);
    let mut scope = vec![false; width * height];
    let mut any = false;

    for y in 0..height {
        for x in 0..width {
            if covered.data[y * width + x] <= 0.0 {
                continue;
            }
            any = true;
            let y0 = y.saturating_sub(dilation);
            let y1 = (y + dilation).min(height - 1);
            let x0 = x.saturating_sub(dilation);
            let x1 = (x + dilation).min(width - 1);
            for yy in y0..=y1 {
                for xx in x0..=x1 {
                    scope[yy * width + xx] = true;
                }
            }
        }
    }

    // A degenerate outline would otherwise scope out every pixel and make all
    // candidates score zero.
    if !any {
        scope.iter_mut().for_each(|flag| *flag = true);
    }
    scope
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Point;
    use crate::path::{Outline, Segment};

    fn square_contour(x: f64, y: f64, size: f64) -> Contour {
        Contour {
            start: Point::new(x, y),
            segments: vec![
                Segment::Line {
                    to: Point::new(x + size, y),
                },
                Segment::Line {
                    to: Point::new(x + size, y + size),
                },
                Segment::Line {
                    to: Point::new(x, y + size),
                },
                Segment::Line {
                    to: Point::new(x, y),
                },
            ],
        }
    }

    #[test]
    fn a_pixel_aligned_square_rasterizes_exactly() {
        let contour = square_contour(2.0, 2.0, 4.0);
        let mask = rasterize(&[contour], (0, 0, 8, 8));

        assert!((mask.total() - 16.0).abs() < 1e-4, "total {}", mask.total());
        for y in 0..8 {
            for x in 0..8 {
                let expected = if (2..6).contains(&x) && (2..6).contains(&y) {
                    1.0
                } else {
                    0.0
                };
                assert!(
                    (mask.get(x, y) - expected).abs() < 1e-4,
                    "pixel ({x},{y}) was {} expected {expected}",
                    mask.get(x, y)
                );
            }
        }
    }

    #[test]
    fn a_half_covered_pixel_column_reports_half_coverage() {
        // Square from x=2.5 to x=6.5: columns 2 and 6 are half covered.
        let contour = square_contour(2.5, 0.0, 4.0);
        let mask = rasterize(&[contour], (0, 0, 8, 4));
        assert!((mask.get(2, 1) - 0.5).abs() < 1e-4, "{}", mask.get(2, 1));
        assert!((mask.get(3, 1) - 1.0).abs() < 1e-4, "{}", mask.get(3, 1));
        assert!((mask.get(6, 1) - 0.5).abs() < 1e-4, "{}", mask.get(6, 1));
    }

    #[test]
    fn circle_area_is_recovered_to_a_fraction_of_a_pixel() {
        let radius = 20.0;
        let outline = Outline::Circle {
            center: Point::new(24.0, 24.0),
            radius,
        };
        let mask = rasterize(&[outline.to_contour()], (0, 0, 48, 48));
        let expected = std::f64::consts::PI * radius * radius;
        let error = (mask.total() - expected).abs();
        assert!(
            error / expected < 1e-3,
            "area {} vs {expected} (error {error})",
            mask.total()
        );
    }

    #[test]
    fn even_odd_fill_leaves_a_hole() {
        let outer = square_contour(0.0, 0.0, 10.0);
        let inner = square_contour(3.0, 3.0, 4.0);
        let mask = rasterize(&[outer, inner], (0, 0, 10, 10));
        assert!((mask.get(5, 5) - 0.0).abs() < 1e-4, "hole should be empty");
        assert!((mask.get(1, 1) - 1.0).abs() < 1e-4, "ring should be filled");
        assert!((mask.total() - (100.0 - 16.0)).abs() < 1e-3);
    }

    #[test]
    fn comparison_reports_zero_for_a_perfect_match() {
        let mut target = Field::new(8, 8);
        for y in 2..6 {
            for x in 2..6 {
                target.set(x, y, 1.0);
            }
        }
        let mask = rasterize(&[square_contour(2.0, 2.0, 4.0)], (0, 0, 8, 8));
        let comparison = compare(&mask, &target);
        assert!(comparison.mean_error < 1e-4, "{comparison:?}");
        assert!(comparison.max_error < 1e-4, "{comparison:?}");
        assert!(comparison.area_delta.abs() < 1e-3, "{comparison:?}");
    }

    #[test]
    fn comparison_notices_a_half_pixel_shift() {
        let mut target = Field::new(8, 8);
        for y in 2..6 {
            for x in 2..6 {
                target.set(x, y, 1.0);
            }
        }
        let mask = rasterize(&[square_contour(2.5, 2.0, 4.0)], (0, 0, 8, 8));
        let comparison = compare(&mask, &target);
        assert!(
            comparison.max_error > 0.4,
            "a half pixel shift should be visible: {comparison:?}"
        );
    }
}
