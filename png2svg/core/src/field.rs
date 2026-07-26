//! Sub-pixel coverage fields.
//!
//! An anti-aliased pixel is not a color that needs a palette slot; it is a
//! *measurement* of how much of that pixel each real color covers. Compositing
//! is linear in premultiplied-alpha space, so a pixel sitting on a boundary
//! between palette colors `A` and `B` satisfies
//!
//! ```text
//! pixel = t * A + (1 - t) * B
//! ```
//!
//! and `t` is exactly the fraction of the pixel covered by `A`. Recovering `t`
//! turns every anti-aliased edge pixel into a sub-pixel position sample, which
//! is what lets the tracer place a boundary to a fraction of a pixel instead of
//! snapping it to the integer grid.

use image::RgbaImage;

use crate::quantize::{Palette, Premul};

/// A scalar field sampled at pixel centres.
///
/// Sample `(x, y)` corresponds to image coordinate `(x + 0.5, y + 0.5)`.
#[derive(Debug, Clone)]
pub struct Field {
    pub width: usize,
    pub height: usize,
    pub data: Vec<f32>,
}

impl Field {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            data: vec![0.0; width * height],
        }
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> f32 {
        self.data[y * self.width + x]
    }

    #[inline]
    pub fn set(&mut self, x: usize, y: usize, value: f32) {
        self.data[y * self.width + x] = value;
    }

    /// Sum of all samples, i.e. the total area the field covers in pixels.
    pub fn total(&self) -> f64 {
        self.data.iter().map(|&value| value as f64).sum()
    }

    pub fn max(&self) -> f32 {
        self.data.iter().copied().fold(0.0, f32::max)
    }
}

/// Per-palette-color coverage fields plus the hard label map.
pub struct Decomposition {
    /// One field per palette entry; the fields sum to 1.0 at every pixel.
    pub coverage: Vec<Field>,
    /// Index of the dominant palette entry at each pixel.
    pub labels: Vec<u16>,
    pub width: usize,
    pub height: usize,
}

/// How many of the nearest palette entries are tried as the primary of a blend.
///
/// Fixing the primary to the single *nearest* entry is wrong, and wrong in a way
/// that matters. A half-covered red pixel over transparency sits at the midpoint
/// of the segment `[transparent, red]`, and that midpoint can easily be closer to
/// some third color in the palette than to either end — a 50% red is numerically
/// nearer to a dark navy than it is to red or to nothing. Committing to that
/// third color then attributes the whole anti-aliased rim of a red mark to navy,
/// which shreds the boundary. Trying several starting points costs a handful of
/// arithmetic per pixel and removes the failure entirely.
const BLEND_PRIMARIES: usize = 4;

/// Decompose the image into per-color coverage.
///
/// Each pixel is explained as a blend of two palette entries: the pair whose
/// connecting segment passes closest to the pixel's premultiplied color, with
/// the position along that segment giving the coverage split.
pub fn decompose(image: &RgbaImage, palette: &Palette) -> Decomposition {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let entry_count = palette.len().max(1);

    let mut coverage: Vec<Field> = (0..entry_count)
        .map(|_| Field::new(width, height))
        .collect();
    let mut labels = vec![0u16; width * height];

    for (index, pixel) in image.pixels().enumerate() {
        let sample = Premul::from_rgba(pixel.0);

        let Some(blend) = explain(&palette.premul, sample) else {
            continue;
        };

        coverage[blend.primary].data[index] = blend.weight;
        if let Some(partner) = blend.partner {
            coverage[partner].data[index] = 1.0 - blend.weight;
        } else {
            // Nothing to blend against, so the primary color owns the pixel.
            coverage[blend.primary].data[index] = 1.0;
        }

        labels[index] = if blend.weight >= 0.5 {
            blend.primary as u16
        } else {
            blend.partner.unwrap_or(blend.primary) as u16
        };
    }

    Decomposition {
        coverage,
        labels,
        width,
        height,
    }
}

struct Blend {
    primary: usize,
    partner: Option<usize>,
    /// Share of the pixel belonging to `primary`.
    weight: f32,
}

/// Best two-entry explanation of `sample`.
fn explain(entries: &[Premul], sample: Premul) -> Option<Blend> {
    if entries.is_empty() {
        return None;
    }

    // The nearest few entries, cheaply, without sorting the whole palette.
    let mut nearest: [(f32, usize); BLEND_PRIMARIES] = [(f32::INFINITY, usize::MAX); BLEND_PRIMARIES];
    for (index, &entry) in entries.iter().enumerate() {
        let distance = sample.distance_sq(entry);
        if distance >= nearest[BLEND_PRIMARIES - 1].0 {
            continue;
        }
        let mut slot = BLEND_PRIMARIES - 1;
        while slot > 0 && nearest[slot - 1].0 > distance {
            nearest[slot] = nearest[slot - 1];
            slot -= 1;
        }
        nearest[slot] = (distance, index);
    }

    let (best_distance, best_index) = nearest[0];
    if best_index == usize::MAX {
        return None;
    }

    // Baseline: the nearest entry alone, unblended.
    let mut best = Blend {
        primary: best_index,
        partner: None,
        weight: 1.0,
    };
    let mut best_residual = best_distance;

    for &(_, primary) in nearest.iter() {
        if primary == usize::MAX {
            continue;
        }
        let primary_color = entries[primary];

        for (partner, &candidate) in entries.iter().enumerate() {
            if partner == primary {
                continue;
            }

            let axis = primary_color.sub(candidate);
            let axis_length_sq = axis.dot(axis);
            if axis_length_sq < 1e-9 {
                continue;
            }

            // Position of the sample projected onto the segment.
            let weight = (sample.sub(candidate).dot(axis) / axis_length_sq).clamp(0.0, 1.0);
            let blended = Premul {
                r: candidate.r + axis.r * weight,
                g: candidate.g + axis.g * weight,
                b: candidate.b + axis.b * weight,
                a: candidate.a + axis.a * weight,
            };
            let residual = sample.distance_sq(blended);

            // Require a real improvement, so a pixel sitting exactly on a
            // palette color is not split across a spurious partner.
            if residual < best_residual - 1e-9 {
                best_residual = residual;
                best = Blend {
                    primary,
                    partner: Some(partner),
                    weight,
                };
            }
        }
    }

    Some(best)
}

/// A binary mask of the pixels whose dominant color is `entry`.
pub fn label_mask(decomposition: &Decomposition, entry: usize) -> Vec<bool> {
    decomposition
        .labels
        .iter()
        .map(|&label| label as usize == entry)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quantize::build_palette;
    use image::Rgba;

    #[test]
    fn a_half_covered_edge_pixel_reports_half_coverage() {
        // Two solid fields with a single column of 50/50 blend between them,
        // which is what a vertical edge looks like after anti-aliasing.
        let image = RgbaImage::from_fn(12, 4, |x, _| match x.cmp(&5) {
            std::cmp::Ordering::Less => Rgba([255, 0, 0, 255]),
            std::cmp::Ordering::Equal => Rgba([128, 0, 127, 255]),
            std::cmp::Ordering::Greater => Rgba([0, 0, 255, 255]),
        });
        let palette = build_palette(&image, 8, 0);
        assert_eq!(palette.len(), 2, "palette was {:?}", palette.colors);

        let red = palette
            .colors
            .iter()
            .position(|c| c[0] > 200)
            .expect("red is in the palette");
        let decomposition = decompose(&image, &palette);

        let coverage = &decomposition.coverage[red];
        assert!((coverage.get(2, 2) - 1.0).abs() < 1e-4);
        assert!(
            (coverage.get(5, 2) - 0.5).abs() < 0.02,
            "blend pixel reported {}",
            coverage.get(5, 2)
        );
        assert!(coverage.get(8, 2).abs() < 1e-4);
    }

    #[test]
    fn alpha_edges_become_fractional_coverage() {
        let image = RgbaImage::from_fn(12, 4, |x, _| match x.cmp(&5) {
            std::cmp::Ordering::Less => Rgba([0, 0, 0, 255]),
            std::cmp::Ordering::Equal => Rgba([0, 0, 0, 64]),
            std::cmp::Ordering::Greater => Rgba([0, 0, 0, 0]),
        });
        let palette = build_palette(&image, 8, 0);
        let ink = palette
            .colors
            .iter()
            .position(|c| c[3] == 255)
            .expect("opaque ink is in the palette");
        let decomposition = decompose(&image, &palette);

        let coverage = &decomposition.coverage[ink];
        assert!((coverage.get(2, 2) - 1.0).abs() < 1e-4);
        assert!(
            (coverage.get(5, 2) - 64.0 / 255.0).abs() < 0.02,
            "alpha 64 reported {}",
            coverage.get(5, 2)
        );
        assert!(coverage.get(8, 2).abs() < 1e-4);
    }

    #[test]
    fn coverage_sums_to_one_everywhere() {
        let image = RgbaImage::from_fn(16, 16, |x, y| {
            let inside = (x as i32 - 8).pow(2) + (y as i32 - 8).pow(2) < 30;
            if inside {
                Rgba([220, 30, 30, 255])
            } else if (x + y) % 3 == 0 {
                Rgba([220, 30, 30, 120])
            } else {
                Rgba([20, 40, 200, 255])
            }
        });
        let palette = build_palette(&image, 8, 0);
        let decomposition = decompose(&image, &palette);

        for index in 0..(16 * 16) {
            let total: f32 = decomposition
                .coverage
                .iter()
                .map(|field| field.data[index])
                .sum();
            assert!(
                (total - 1.0).abs() < 1e-4,
                "coverage at {index} summed to {total}"
            );
        }
    }

    #[test]
    fn total_coverage_matches_the_painted_area() {
        // A 4x4 opaque block inside an 8x8 transparent canvas.
        let image = RgbaImage::from_fn(8, 8, |x, y| {
            if (2..6).contains(&x) && (2..6).contains(&y) {
                Rgba([10, 10, 10, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let palette = build_palette(&image, 4, 0);
        let ink = palette
            .colors
            .iter()
            .position(|c| c[3] == 255)
            .expect("ink is in the palette");
        let decomposition = decompose(&image, &palette);
        assert!((decomposition.coverage[ink].total() - 16.0).abs() < 1e-3);
    }
}
