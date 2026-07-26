//! Palette construction.
//!
//! The important difference from a naive quantizer is that the palette is built
//! from *interior* pixels only. Anti-aliased edge pixels form a continuum
//! between the real colors of an image; if they are fed to the quantizer they
//! steal palette slots and produce phantom "halo" colors along every boundary.
//! Excluding them first means the palette describes the colors a designer
//! actually used, and the edge pixels are then explained as blends of those
//! colors by [`crate::field`].

use std::collections::HashMap;

use image::RgbaImage;

/// A color in premultiplied-alpha space, each channel normalized to `0.0..=1.0`.
///
/// Compositing is linear in this space, which is what lets the coverage stage
/// recover sub-pixel edge positions from anti-aliased pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Premul {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

// Named methods rather than operator traits, matching `geom::Point`.
#[allow(clippy::should_implement_trait)]
impl Premul {
    pub fn from_rgba(rgba: [u8; 4]) -> Self {
        let alpha = rgba[3] as f32 / 255.0;
        Self {
            r: rgba[0] as f32 / 255.0 * alpha,
            g: rgba[1] as f32 / 255.0 * alpha,
            b: rgba[2] as f32 / 255.0 * alpha,
            a: alpha,
        }
    }

    pub fn distance_sq(self, other: Premul) -> f32 {
        let dr = self.r - other.r;
        let dg = self.g - other.g;
        let db = self.b - other.b;
        let da = self.a - other.a;
        dr * dr + dg * dg + db * db + da * da
    }

    pub fn sub(self, other: Premul) -> Premul {
        Premul {
            r: self.r - other.r,
            g: self.g - other.g,
            b: self.b - other.b,
            a: self.a - other.a,
        }
    }

    pub fn dot(self, other: Premul) -> f32 {
        self.r * other.r + self.g * other.g + self.b * other.b + self.a * other.a
    }
}

#[derive(Debug, Clone)]
pub struct Palette {
    pub colors: Vec<[u8; 4]>,
    pub premul: Vec<Premul>,
    /// Index of the fully transparent entry, when the image has transparency.
    pub transparent: Option<usize>,
}

impl Palette {
    fn new(colors: Vec<[u8; 4]>) -> Self {
        let transparent = colors.iter().position(|c| c[3] == 0);
        let premul = colors.iter().copied().map(Premul::from_rgba).collect();
        Self {
            colors,
            premul,
            transparent,
        }
    }

    pub fn len(&self) -> usize {
        self.colors.len()
    }

    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }

    /// Opaque entries, i.e. everything that will actually be drawn.
    pub fn opaque_indices(&self) -> impl Iterator<Item = usize> + '_ {
        (0..self.colors.len()).filter(move |&i| self.colors[i][3] > 0)
    }
}

/// Perceptually weighted squared distance between 8-bit colors.
///
/// Uses the classic low-cost green-weighted approximation, with alpha weighted
/// heavily because an alpha mismatch is far more visible than a hue shift.
pub fn perceptual_distance_sq(a: [u8; 4], b: [u8; 4]) -> u32 {
    let dr = a[0] as i32 - b[0] as i32;
    let dg = a[1] as i32 - b[1] as i32;
    let db = a[2] as i32 - b[2] as i32;
    let da = a[3] as i32 - b[3] as i32;
    (2 * dr * dr + 4 * dg * dg + 3 * db * db + 8 * da * da) as u32
}

/// Classify every pixel as interior (safely inside a flat region) or not.
///
/// A pixel is interior when it and all four of its neighbors carry essentially
/// the same premultiplied color. Anti-aliased edges, gradients and noise all
/// fail this test.
pub fn interior_mask(image: &RgbaImage) -> Vec<bool> {
    let width = image.width() as usize;
    let height = image.height() as usize;
    let premul: Vec<Premul> = image
        .pixels()
        .map(|pixel| Premul::from_rgba(pixel.0))
        .collect();

    // ~4/255 per channel; tight enough to reject anti-aliasing, loose enough to
    // tolerate PNG-level dithering and mild compression noise.
    const FLAT_THRESHOLD: f32 = 0.0025;

    let mut mask = vec![false; width * height];
    for y in 0..height {
        for x in 0..width {
            let index = y * width + x;
            let center = premul[index];
            if center.a <= 0.0 {
                continue;
            }

            let mut flat = true;
            if x > 0 && center.distance_sq(premul[index - 1]) > FLAT_THRESHOLD {
                flat = false;
            }
            if flat && x + 1 < width && center.distance_sq(premul[index + 1]) > FLAT_THRESHOLD {
                flat = false;
            }
            if flat && y > 0 && center.distance_sq(premul[index - width]) > FLAT_THRESHOLD {
                flat = false;
            }
            if flat
                && y + 1 < height
                && center.distance_sq(premul[index + width]) > FLAT_THRESHOLD
            {
                flat = false;
            }
            mask[index] = flat;
        }
    }
    mask
}

/// Build a palette of at most `max_colors` opaque entries, plus a transparent
/// entry when the image needs one.
pub fn build_palette(image: &RgbaImage, max_colors: usize, merge_threshold: u32) -> Palette {
    let has_transparency = image.pixels().any(|pixel| pixel.0[3] < 255);
    let opaque_slots = max_colors.max(1);

    let histogram = build_histogram(image);
    if histogram.is_empty() {
        return Palette::new(vec![[0, 0, 0, 0]]);
    }

    let groups = median_cut(histogram, opaque_slots);
    let mut colors: Vec<[u8; 4]> = merge_similar(groups, merge_threshold)
        .iter()
        .map(Bucket::average)
        .collect();
    colors.sort_unstable();

    if has_transparency {
        colors.push([0, 0, 0, 0]);
    }

    Palette::new(colors)
}

/// Accumulated color mass for one histogram bucket.
#[derive(Debug, Clone, Copy)]
struct Bucket {
    sum: [u64; 4],
    count: u64,
}

impl Bucket {
    fn average(&self) -> [u8; 4] {
        if self.count == 0 {
            return [0, 0, 0, 0];
        }
        let mut out = [0u8; 4];
        for (slot, &sum) in out.iter_mut().zip(self.sum.iter()) {
            // Round to nearest rather than truncating; over a whole logo the
            // half-step bias is visible as a slight darkening.
            let value = (sum + self.count / 2) / self.count;
            *slot = value.min(255) as u8;
        }
        out
    }

    fn premul(&self) -> Premul {
        Premul::from_rgba(self.average())
    }
}

/// Histogram of interior colors, keyed on a lightly quantized color so the map
/// stays bounded on photographic input. Sums keep full 8-bit precision, so the
/// palette entries themselves are exact averages.
fn build_histogram(image: &RgbaImage) -> Vec<Bucket> {
    let interior = interior_mask(image);

    let opaque_count = image.pixels().filter(|pixel| pixel.0[3] > 0).count();
    if opaque_count == 0 {
        return Vec::new();
    }

    // Three progressively looser samples of "pixels whose color is real".
    // Strict interior is best, but thin strokes and gradients may not have
    // enough of it to describe the artwork, so fall back rather than quantize
    // from a handful of samples.
    let strict = |index: usize, rgba: [u8; 4]| rgba[3] > 0 && interior[index];
    let fully_opaque = |_: usize, rgba: [u8; 4]| rgba[3] == 255;
    let any_opaque = |_: usize, rgba: [u8; 4]| rgba[3] > 0;

    let count_matching = |predicate: &dyn Fn(usize, [u8; 4]) -> bool| {
        image
            .pixels()
            .enumerate()
            .filter(|(index, pixel)| predicate(*index, pixel.0))
            .count()
    };

    // A sample is usable when it has enough pixels to quantize and covers a
    // meaningful share of what is actually painted.
    let usable = |count: usize| count >= 8 && count * 4 >= opaque_count;

    let predicates: [&dyn Fn(usize, [u8; 4]) -> bool; 3] =
        [&strict, &fully_opaque, &any_opaque];
    let chosen = predicates
        .iter()
        .find(|predicate| usable(count_matching(**predicate)))
        .copied()
        .unwrap_or(&any_opaque);

    let mut map: HashMap<[u8; 4], Bucket> = HashMap::new();
    for (index, pixel) in image.pixels().enumerate() {
        if !chosen(index, pixel.0) {
            continue;
        }
        // Key on a lightly quantized color so the map stays bounded on
        // photographic input; the sums keep full 8-bit precision, so palette
        // entries are still exact averages.
        let rgba = pixel.0;
        let key = [rgba[0] >> 1, rgba[1] >> 1, rgba[2] >> 1, rgba[3] >> 1];
        let bucket = map.entry(key).or_insert(Bucket {
            sum: [0; 4],
            count: 0,
        });
        for (sum, &value) in bucket.sum.iter_mut().zip(rgba.iter()) {
            *sum += value as u64;
        }
        bucket.count += 1;
    }

    let mut buckets: Vec<Bucket> = map.into_values().collect();
    // Deterministic ordering: HashMap iteration order is not stable.
    buckets.sort_unstable_by_key(|bucket| bucket.average());
    buckets
}

struct ColorBox {
    buckets: Vec<Bucket>,
    population: u64,
    widest_axis: usize,
    extent: f32,
}

impl ColorBox {
    fn new(buckets: Vec<Bucket>) -> Self {
        let population = buckets.iter().map(|bucket| bucket.count).sum();
        let mut min = [f32::INFINITY; 4];
        let mut max = [f32::NEG_INFINITY; 4];
        for bucket in &buckets {
            let premul = bucket.premul();
            let channels = [premul.r, premul.g, premul.b, premul.a];
            for axis in 0..4 {
                min[axis] = min[axis].min(channels[axis]);
                max[axis] = max[axis].max(channels[axis]);
            }
        }

        let mut widest_axis = 0;
        let mut extent = 0.0;
        for axis in 0..4 {
            let range = max[axis] - min[axis];
            if range > extent {
                extent = range;
                widest_axis = axis;
            }
        }

        Self {
            buckets,
            population,
            widest_axis,
            extent,
        }
    }

    /// Splitting priority: a big box holding many pixels is the one whose
    /// error dominates the result.
    fn priority(&self) -> f64 {
        if self.buckets.len() < 2 || self.extent <= 0.0 {
            return 0.0;
        }
        self.extent as f64 * (self.population as f64).sqrt()
    }

    /// Total colour mass in this box: the representative colour together with
    /// how many pixels voted for it.
    fn mass(&self) -> Bucket {
        let mut sum = [0u64; 4];
        let mut count = 0u64;
        for bucket in &self.buckets {
            for (slot, &value) in sum.iter_mut().zip(bucket.sum.iter()) {
                *slot += value;
            }
            count += bucket.count;
        }
        Bucket { sum, count }
    }

    fn split(mut self) -> (ColorBox, ColorBox) {
        let axis = self.widest_axis;
        self.buckets.sort_unstable_by(|a, b| {
            let pa = channel_of(a.premul(), axis);
            let pb = channel_of(b.premul(), axis);
            pa.partial_cmp(&pb)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then_with(|| a.average().cmp(&b.average()))
        });

        // Split at the population median so both halves carry similar weight.
        let half = self.population / 2;
        let mut running = 0u64;
        let mut split_at = 0usize;
        for (index, bucket) in self.buckets.iter().enumerate() {
            running += bucket.count;
            if running >= half {
                split_at = index;
                break;
            }
        }
        let split_at = split_at.clamp(0, self.buckets.len().saturating_sub(2));

        let right = self.buckets.split_off(split_at + 1);
        (ColorBox::new(self.buckets), ColorBox::new(right))
    }
}

fn channel_of(premul: Premul, axis: usize) -> f32 {
    match axis {
        0 => premul.r,
        1 => premul.g,
        2 => premul.b,
        _ => premul.a,
    }
}

/// Returns one bucket per palette slot, keeping the accumulated colour mass.
///
/// The populations matter downstream: merging has to know that a 300-pixel blend
/// weighs far less than a 30,000-pixel fill.
fn median_cut(buckets: Vec<Bucket>, max_colors: usize) -> Vec<Bucket> {
    if buckets.is_empty() {
        return vec![Bucket {
            sum: [0; 4],
            count: 1,
        }];
    }

    let mut boxes = vec![ColorBox::new(buckets)];
    while boxes.len() < max_colors {
        let candidate = boxes
            .iter()
            .enumerate()
            .filter(|(_, color_box)| color_box.priority() > 0.0)
            .max_by(|(_, a), (_, b)| {
                a.priority()
                    .partial_cmp(&b.priority())
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|(index, _)| index);

        let Some(index) = candidate else {
            break;
        };

        let (left, right) = boxes.swap_remove(index).split();
        boxes.push(left);
        boxes.push(right);
    }

    boxes
        .iter()
        .filter(|color_box| !color_box.buckets.is_empty())
        .map(ColorBox::mass)
        .collect()
}

/// Collapse palette entries that no viewer could tell apart.
///
/// Logos routinely carry a handful of near-identical brand colors introduced by
/// resampling, and a noisy source spreads one flat fill across several palette
/// slots. Left alone, those duplicates fragment a single region into hundreds of
/// shapes, because each pixel picks whichever near-twin it happens to be closest
/// to.
///
/// This is proper agglomerative clustering — repeatedly merge the closest pair
/// of groups — rather than one sequential pass. A sequential pass depends on the
/// order the quantizer happened to emit: given colors 197, 203, 199 and 201, it
/// compares 203 against 197, finds them too far apart, and strands them in
/// separate groups even though 199 and 201 would have chained them together.
///
/// Populations are carried through the merge, and that is essential rather than
/// a refinement. A low-contrast boundary — cream artwork on a white page — puts
/// its blend colors into the palette, and those blends form a chain of small
/// steps between the two real colors. Merging on unweighted averages walks that
/// chain and drags both ends to the midpoint, so the page and the artwork
/// collapse into one phantom colour and the boundary between them dissolves into
/// noise. Weighting by population pins each group to whichever populous colour it
/// belongs to, and the two real colors stay the required distance apart.
fn merge_similar(groups: Vec<Bucket>, threshold: u32) -> Vec<Bucket> {
    if threshold == 0 || groups.len() < 2 {
        return groups;
    }

    let mut groups = groups;
    loop {
        // Closest pair of group averages still within the threshold.
        let mut best: Option<(usize, usize, u32)> = None;
        for left in 0..groups.len() {
            for right in (left + 1)..groups.len() {
                let distance =
                    perceptual_distance_sq(groups[left].average(), groups[right].average());
                if distance <= threshold
                    && best.is_none_or(|(_, _, closest)| distance < closest)
                {
                    best = Some((left, right, distance));
                }
            }
        }

        let Some((left, right, _)) = best else {
            break;
        };

        let absorbed = groups.remove(right);
        for (slot, &value) in groups[left].sum.iter_mut().zip(absorbed.sum.iter()) {
            *slot += value;
        }
        groups[left].count += absorbed.count;
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn solid_with_aa_edge() -> RgbaImage {
        // Left half red, right half blue, with one column of 50/50 blend in
        // between - the classic anti-aliased boundary.
        RgbaImage::from_fn(9, 8, |x, _| match x.cmp(&4) {
            std::cmp::Ordering::Less => Rgba([200, 30, 30, 255]),
            std::cmp::Ordering::Equal => Rgba([115, 30, 115, 255]),
            std::cmp::Ordering::Greater => Rgba([30, 30, 200, 255]),
        })
    }

    #[test]
    fn interior_mask_rejects_the_anti_aliased_seam() {
        let image = solid_with_aa_edge();
        let mask = interior_mask(&image);
        let width = image.width() as usize;

        // Column 4 is the blend, columns 3 and 5 touch it.
        for y in 0..image.height() as usize {
            assert!(!mask[y * width + 3], "column 3 should not be interior");
            assert!(!mask[y * width + 4], "column 4 should not be interior");
            assert!(!mask[y * width + 5], "column 5 should not be interior");
            assert!(mask[y * width + 1], "column 1 should be interior");
            assert!(mask[y * width + 7], "column 7 should be interior");
        }
    }

    #[test]
    fn palette_ignores_blend_colors() {
        let image = solid_with_aa_edge();
        let palette = build_palette(&image, 8, 0);

        assert_eq!(
            palette.len(),
            2,
            "expected only the two real colors, got {:?}",
            palette.colors
        );
        assert!(palette.colors.contains(&[200, 30, 30, 255]));
        assert!(palette.colors.contains(&[30, 30, 200, 255]));
    }

    #[test]
    fn palette_adds_a_transparent_entry_when_needed() {
        let image = RgbaImage::from_fn(6, 6, |x, y| {
            if (1..5).contains(&x) && (1..5).contains(&y) {
                Rgba([10, 10, 10, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });
        let palette = build_palette(&image, 4, 0);
        assert!(palette.transparent.is_some());
        assert!(palette.colors.contains(&[10, 10, 10, 255]));
    }

    #[test]
    fn merge_collapses_near_duplicate_brand_colors() {
        let image = RgbaImage::from_fn(8, 4, |x, _| {
            if x < 4 {
                Rgba([220, 40, 40, 255])
            } else {
                Rgba([225, 43, 39, 255])
            }
        });
        let palette = build_palette(&image, 8, 400);
        assert_eq!(palette.len(), 1, "got {:?}", palette.colors);
    }

    #[test]
    fn low_contrast_neighbours_survive_the_merge() {
        // Cream artwork on a white page: the two colors are only ~9/255 apart,
        // and the soft edge between them steps by less than the flatness
        // threshold, so its blend colors do reach the palette. Merging on
        // unweighted averages then walks that chain and drags both ends to the
        // midpoint, collapsing page and artwork into one phantom colour. Carrying
        // populations pins each blend to the populous colour it belongs to.
        let image = RgbaImage::from_fn(64, 16, |x, _| {
            let t = ((x as f32 - 30.0) / 4.0).clamp(0.0, 1.0);
            let ramp = |from: f32, to: f32| (from + (to - from) * t).round() as u8;
            Rgba([
                ramp(246.0, 255.0),
                ramp(246.0, 255.0),
                ramp(244.0, 255.0),
                255,
            ])
        });

        let palette = build_palette(&image, 8, 196);

        let cream = palette
            .colors
            .iter()
            .find(|color| color[0] < 251)
            .copied()
            .unwrap_or_else(|| panic!("cream was lost: {:?}", palette.colors));
        let page = palette
            .colors
            .iter()
            .find(|color| color[0] >= 253)
            .copied()
            .unwrap_or_else(|| panic!("the page was lost: {:?}", palette.colors));

        // Both must stay close to where they started, rather than meeting in the
        // middle.
        assert!(
            perceptual_distance_sq(cream, [246, 246, 244, 255]) < 100,
            "cream drifted to {cream:?}"
        );
        assert!(
            perceptual_distance_sq(page, [255, 255, 255, 255]) < 100,
            "page drifted to {page:?}"
        );
    }

    #[test]
    fn palette_is_deterministic() {
        let image = solid_with_aa_edge();
        let first = build_palette(&image, 8, 0);
        for _ in 0..8 {
            let next = build_palette(&image, 8, 0);
            assert_eq!(first.colors, next.colors);
        }
    }
}
