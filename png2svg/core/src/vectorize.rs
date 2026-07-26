//! The vectorization pipeline.
//!
//! ```text
//! image
//!   -> palette from interior pixels only          (quantize)
//!   -> per-color sub-pixel coverage fields        (field)
//!   -> contours at the 0.5 coverage isoline       (trace)
//!   -> corners, straight runs, primitives         (corner, fit, primitive)
//!   -> candidate outlines, coarse to fine
//!   -> rasterize each candidate, compare to the source coverage
//!   -> keep the simplest candidate that measures well enough
//!   -> SVG
//! ```
//!
//! The last three steps are the part that matters. Nothing here guesses whether
//! a region "is" a circle or whether a curve fit is "smooth enough": every
//! candidate is rendered back to coverage and scored against what was measured
//! from the image, and the simplest candidate that survives wins. That makes the
//! quality/complexity trade-off an optimization rather than a tuning exercise.

use image::RgbaImage;

use crate::corner::{detect_corners, CornerConfig};
use crate::field::{decompose, label_mask, Field};
use crate::geom::{signed_area, Bounds, Point};
use crate::path::{Contour, Outline, Shape};
use crate::primitive::{detect_rect, fit_circle, fit_ellipse};
use crate::quantize::build_palette;
use crate::raster::{compare_within, rasterize, scope_of, window_for};
use crate::svg::Document;
use crate::trace::{nest, trace_cells, trace_level};
use crate::{VectorizeMode, VectorizeOptions};

/// Fit tolerances tried for each contour, as multiples of the plan's tolerance.
///
/// The requested tolerance is a ceiling: the ladder only ever refines past it,
/// never below the accuracy the caller asked for.
const TOLERANCE_LADDER: [f64; 4] = [1.0, 0.5, 0.25, 0.125];

/// A candidate is accepted when its measured error is within this factor of the
/// best error any candidate achieved. Keeps the engine from paying for nodes
/// that buy no visible accuracy.
const RELATIVE_SLACK: f64 = 1.3;

/// Resolved, mode-specific parameters for one run.
#[derive(Debug, Clone)]
struct Plan {
    palette_size: usize,
    merge_threshold: u32,
    /// Maximum geometric fitting error, in pixels.
    fit_tolerance: f64,
    /// Coverage error accepted outright, without needing the relative test.
    error_budget: f64,
    /// Smallest contour area kept, in pixels.
    min_area: f64,
    corners: CornerConfig,
    /// Trace exact cell edges and skip curve fitting entirely (pixel art).
    crisp: bool,
    detect_primitives: bool,
    refine: bool,
}

fn plan_for(options: &VectorizeOptions) -> Plan {
    let detail = options.detail.clamp(0.0, 1.0) as f64;
    let smoothness = options.smoothness.clamp(0.0, 1.0) as f64;
    let crisp = options.mode == VectorizeMode::PixelArt;

    // `tolerance` is quoted in the CLI's 0.1..10 range; a quarter of it maps to
    // a sensible sub-pixel error budget with 1.5 (the default) giving ~0.38px.
    let fit_tolerance = if crisp {
        0.0
    } else {
        (options.tolerance as f64 * 0.25).clamp(0.05, 2.0)
    };

    Plan {
        palette_size: options.colors.max(2) as usize,
        merge_threshold: palette_merge_threshold(options),
        fit_tolerance,
        error_budget: (fit_tolerance * 1.2).clamp(0.06, 0.5),
        // Higher detail keeps smaller specks. A single anti-aliased pixel
        // traces to an area of about 0.5, so the useful range straddles that.
        min_area: if crisp {
            0.0
        } else {
            (4.0 * (1.0 - detail)).max(0.0)
        },
        corners: CornerConfig {
            // Smoother settings demand stronger evidence before breaking a
            // curve with a corner.
            min_angle: 0.28 + smoothness * 0.22,
            min_ratio: 0.66 + smoothness * 0.10,
            ..CornerConfig::default()
        },
        crisp,
        detect_primitives: !crisp,
        refine: !crisp,
    }
}

fn palette_merge_threshold(options: &VectorizeOptions) -> u32 {
    match options.mode {
        VectorizeMode::PixelArt => 0,
        VectorizeMode::Auto | VectorizeMode::Logo => {
            if options.detail >= 0.85 {
                64
            } else if options.detail >= 0.55 {
                196
            } else {
                400
            }
        }
        VectorizeMode::Poster => {
            if options.detail >= 0.8 {
                144
            } else {
                324
            }
        }
    }
}

/// Run the full pipeline.
pub fn vectorize(image: &RgbaImage, options: &VectorizeOptions) -> Document {
    let width = image.width();
    let height = image.height();
    let plan = plan_for(options);

    let mut document = Document {
        width,
        height,
        shapes: Vec::new(),
    };
    if width == 0 || height == 0 {
        return document;
    }

    let palette = build_palette(image, plan.palette_size, plan.merge_threshold);
    let decomposition = decompose(image, &palette);

    // Collected per color so paint order can put the largest colors underneath.
    let mut per_color: Vec<(f64, [u8; 4], Vec<Shape>)> = Vec::new();

    for entry in palette.opaque_indices() {
        let color = palette.colors[entry];
        let coverage = &decomposition.coverage[entry];

        let rings = if plan.crisp {
            let mask = label_mask(&decomposition, entry);
            trace_cells(&mask, width as usize, height as usize)
        } else {
            trace_level(coverage, 0.5)
        };

        let rings: Vec<Vec<Point>> = rings
            .into_iter()
            .filter(|ring| signed_area(ring).abs() >= plan.min_area && ring.len() >= 3)
            .collect();
        if rings.is_empty() {
            continue;
        }

        let nesting = nest(&rings);
        let mut shapes = Vec::new();
        for (slot, &outer_index) in nesting.outers.iter().enumerate() {
            let hole_rings: Vec<&[Point]> = nesting.holes[slot]
                .iter()
                .map(|&index| rings[index].as_slice())
                .collect();

            if let Some(shape) = build_shape(
                color,
                &rings[outer_index],
                &hole_rings,
                coverage,
                width as usize,
                height as usize,
                &plan,
            ) {
                shapes.push(shape);
            }
        }

        if shapes.is_empty() {
            continue;
        }

        // Largest shapes first within a color.
        shapes.sort_by(|a, b| {
            b.area()
                .partial_cmp(&a.area())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        let total = coverage.total();
        per_color.push((total, color, shapes));
    }

    // Colors covering more of the canvas paint first, so detail lands on top.
    per_color.sort_by(|a, b| {
        b.0.partial_cmp(&a.0)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.1.cmp(&b.1))
    });

    for (_, _, shapes) in per_color {
        document.shapes.extend(shapes);
    }

    document
}

/// Build one shape, choosing outlines by rendering candidates and measuring them
/// against the coverage the image actually shows.
fn build_shape(
    color: [u8; 4],
    outer_ring: &[Point],
    hole_rings: &[&[Point]],
    target: &Field,
    canvas_width: usize,
    canvas_height: usize,
    plan: &Plan,
) -> Option<Shape> {
    let outer_candidates = candidates_for(outer_ring, plan);
    if outer_candidates.is_empty() {
        return None;
    }
    let hole_candidates: Vec<Candidates> = hole_rings
        .iter()
        .map(|ring| candidates_for(ring, plan))
        .collect();

    // Without refinement, take the simplest candidate for everything and trust
    // the fit. This is the pixel-art path, where there is nothing to choose.
    if !plan.refine {
        return Some(Shape {
            color,
            outer: outer_candidates.outlines[0].clone(),
            holes: hole_candidates
                .iter()
                .filter_map(|candidates| candidates.outlines.first().cloned())
                .collect(),
        });
    }

    // Compare over the shape's own footprint, padded so geometry that renders
    // slightly too large is still penalised instead of being clipped away.
    let mut bounds = outer_candidates
        .outlines
        .iter()
        .fold(Bounds::empty(), |accumulated, candidate| {
            accumulated.union(candidate.bounds())
        });
    bounds = bounds.expand(2.0);
    let window = window_for(bounds, canvas_width, canvas_height);
    if window.2 == 0 || window.3 == 0 {
        return None;
    }

    // Start from the most faithful option for every ring, then simplify each in
    // turn while the whole shape still measures well.
    let mut chosen_outer = outer_candidates.reference;
    let mut chosen_holes: Vec<usize> = hole_candidates
        .iter()
        .map(|candidates| candidates.reference)
        .collect();

    // The traced rings, unfitted, define which pixels belong to this shape.
    let traced: Vec<Contour> = std::iter::once(outer_ring)
        .chain(hole_rings.iter().copied())
        .filter_map(Contour::from_polygon)
        .collect();
    let scope = scope_of(&traced, window, SCOPE_DILATION);

    let measure = |outer: usize, holes: &[usize]| -> f64 {
        let mut contours: Vec<Contour> = vec![outer_candidates.outlines[outer].to_contour()];
        for (index, &choice) in holes.iter().enumerate() {
            if let Some(candidate) = hole_candidates[index].outlines.get(choice) {
                contours.push(candidate.to_contour());
            }
        }
        let mask = rasterize(&contours, window);
        compare_within(&mask, target, Some(&scope)).max_error
    };

    let outer_budget = scaled_budget(
        plan.error_budget,
        outer_candidates.outlines[chosen_outer].bounds(),
    );
    chosen_outer = pick_candidate(
        &outer_candidates.outlines,
        |index| measure(index, &chosen_holes),
        outer_budget,
    )
    .unwrap_or(chosen_outer);

    for hole_index in 0..hole_candidates.len() {
        let candidates = &hole_candidates[hole_index];
        if candidates.len() <= 1 {
            continue;
        }
        let budget = scaled_budget(
            plan.error_budget,
            candidates.outlines[candidates.reference].bounds(),
        );
        let mut trial = chosen_holes.clone();
        let picked = pick_candidate(
            &candidates.outlines,
            |index| {
                trial[hole_index] = index;
                measure(chosen_outer, &trial)
            },
            budget,
        );
        if let Some(picked) = picked {
            chosen_holes[hole_index] = picked;
        }
    }

    let holes = chosen_holes
        .iter()
        .enumerate()
        .filter_map(|(index, &choice)| hole_candidates[index].outlines.get(choice).cloned())
        .collect();

    Some(Shape {
        color,
        outer: outer_candidates.outlines[chosen_outer].clone(),
        holes,
    })
}

/// How far beyond its own traced outline a shape is still measured, in pixels.
/// Wide enough to cover the anti-aliased boundary band on both sides.
const SCOPE_DILATION: usize = 2;

/// Largest share of a shape's own smaller dimension that its boundary may be
/// displaced by. Keeps the absolute budget honest at small sizes.
const EXTENT_FRACTION: f64 = 0.04;

/// Tighten the error budget for small shapes.
///
/// Coverage error at a boundary pixel is roughly the geometric displacement in
/// pixels, so a flat budget means "the edge may move this far". Half a pixel is
/// invisible on a 200px mark and is the entire shape on a 4px one — without this
/// a tiny hard-edged square measures as an acceptable circle.
fn scaled_budget(budget: f64, bounds: Bounds) -> f64 {
    if bounds.is_empty() {
        return budget;
    }
    let extent = bounds.width().min(bounds.height());
    // Never go below a floor, or single-pixel details become unfittable.
    budget.min(extent * EXTENT_FRACTION).max(0.03)
}

/// Score every candidate and return the index of the simplest acceptable one.
///
/// Acceptable means "within the absolute budget, or close enough to what the
/// most faithful candidate managed". The second clause matters because there is
/// a floor on how well any vector shape can explain a given raster — the tracer
/// chamfers corners, anti-aliasing is only an estimate — and chasing below that
/// floor buys nodes without buying accuracy.
///
/// The floor comes from the *path* candidates only. Primitives are hypotheses
/// under test; if the floor were the best score overall, then a shape where every
/// candidate scores badly would let the simplest bad candidate set its own pass
/// mark and win — which is how a 4px hard-edged square ends up accepted as a
/// circle.
fn pick_candidate(
    candidates: &[Outline],
    mut measure: impl FnMut(usize) -> f64,
    budget: f64,
) -> Option<usize> {
    if candidates.is_empty() {
        return None;
    }

    let errors: Vec<f64> = (0..candidates.len()).map(&mut measure).collect();
    let floor = errors
        .iter()
        .enumerate()
        .filter(|(index, _)| !candidates[*index].is_primitive())
        .map(|(_, &error)| error)
        .fold(f64::INFINITY, f64::min);
    // With no path candidate to reference, fall back to the best score overall.
    let floor = if floor.is_finite() {
        floor
    } else {
        errors.iter().copied().fold(f64::INFINITY, f64::min)
    };
    if !floor.is_finite() {
        return None;
    }
    let threshold = budget.max(floor * RELATIVE_SLACK);

    // Candidates are ordered simplest first, so the first acceptable one is the
    // cheapest acceptable one.
    let acceptable = (0..candidates.len()).find(|&index| errors[index] <= threshold);
    Some(acceptable.unwrap_or_else(|| {
        // Nothing met the threshold, which can only happen through numerical
        // edge cases; fall back to the most accurate candidate.
        errors
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(index, _)| index)
            .unwrap_or(0)
    }))
}

/// Candidate outlines for one ring, ordered simplest first.
struct Candidates {
    outlines: Vec<Outline>,
    /// The path fitted at the tightest tolerance: the starting choice before
    /// simplification, and the outline whose bounds set the comparison window.
    /// (The acceptance floor is computed separately, in `pick_candidate`, from
    /// whichever path candidate actually measures best.)
    reference: usize,
}

impl Candidates {
    fn single(outline: Option<Outline>) -> Candidates {
        Candidates {
            outlines: outline.into_iter().collect(),
            reference: 0,
        }
    }

    fn is_empty(&self) -> bool {
        self.outlines.is_empty()
    }

    fn len(&self) -> usize {
        self.outlines.len()
    }
}

fn candidates_for(ring: &[Point], plan: &Plan) -> Candidates {
    if plan.crisp || ring.len() < 8 {
        // Crisp mode traces exact cell edges, and a ring this short has too few
        // samples to fit anything meaningful. Either way the polygon is the
        // answer.
        return Candidates::single(Contour::from_polygon(ring).map(Outline::Path));
    }

    let mut candidates: Vec<Outline> = Vec::new();

    if plan.detect_primitives {
        // Cheap algebraic gate before spending a rasterization on a candidate
        // that obviously does not fit.
        let gate = plan.fit_tolerance * 2.0 + 0.25;

        if let Some(fit) = fit_circle(ring) {
            if fit.max_error <= gate && fit.radius >= 0.75 {
                candidates.push(Outline::Circle {
                    center: fit.center,
                    radius: fit.radius,
                });
            }
        }
        if let Some(fit) = fit_ellipse(ring) {
            // Only worth offering when it is meaningfully not a circle.
            let eccentric = (fit.rx - fit.ry).abs() > plan.fit_tolerance;
            if fit.max_error <= gate && eccentric && fit.ry >= 0.75 {
                candidates.push(Outline::Ellipse {
                    center: fit.center,
                    rx: fit.rx,
                    ry: fit.ry,
                    rotation: fit.rotation,
                });
            }
        }
    }

    let corners = detect_corners(ring, &plan.corners);

    for factor in TOLERANCE_LADDER {
        let tolerance = plan.fit_tolerance * factor;
        let contour = crate::fit::fit_contour(ring, &corners, tolerance);
        if contour.segments.len() < 2 {
            continue;
        }

        // A four-line contour may be an axis-aligned rectangle, which is both
        // simpler to express and exactly right.
        if plan.detect_primitives {
            if let Some(rect) = detect_rect(contour.start, &contour.segments, plan.fit_tolerance) {
                if !candidates.contains(&rect) {
                    candidates.push(rect);
                }
            }
        }

        let outline = Outline::Path(contour);
        if !candidates.contains(&outline) {
            candidates.push(outline);
        }
    }

    if candidates.is_empty() {
        return Candidates::single(Contour::from_polygon(ring).map(Outline::Path));
    }

    // The last ladder entry was fitted at the tightest tolerance, so it is the
    // most faithful candidate. Remember it by value, because sorting moves it.
    let reference_outline = candidates[candidates.len() - 1].clone();

    // Simplest first. Stable, so primitives keep priority over equally cheap
    // paths and the whole selection stays deterministic.
    candidates.sort_by_key(|candidate| candidate.node_count());

    let reference = candidates
        .iter()
        .position(|candidate| *candidate == reference_outline)
        .unwrap_or(candidates.len() - 1);

    Candidates {
        outlines: candidates,
        reference,
    }
}

/// Infer a mode and matching parameters from the image itself.
pub fn resolve_auto_options(image: &RgbaImage, options: &VectorizeOptions) -> VectorizeOptions {
    if options.mode != VectorizeMode::Auto {
        return options.clone();
    }

    let stats = analyze(image);
    match infer_mode(image, &stats) {
        VectorizeMode::PixelArt => VectorizeOptions {
            colors: stats.distinct_colors.clamp(2, 16),
            detail: 1.0,
            smoothness: 0.0,
            tolerance: 0.5,
            mode: VectorizeMode::PixelArt,
        },
        VectorizeMode::Poster => VectorizeOptions {
            colors: stats.distinct_colors.clamp(8, 32),
            detail: 0.85,
            smoothness: 0.45,
            tolerance: 1.5,
            mode: VectorizeMode::Poster,
        },
        _ => VectorizeOptions {
            colors: stats.distinct_colors.clamp(2, 12),
            detail: 0.65,
            smoothness: 0.72,
            tolerance: 1.4,
            mode: VectorizeMode::Logo,
        },
    }
}

struct ImageStats {
    opaque_pixels: usize,
    partial_alpha_pixels: usize,
    distinct_colors: u8,
    has_transparency: bool,
}

fn analyze(image: &RgbaImage) -> ImageStats {
    let mut opaque_pixels = 0usize;
    let mut partial_alpha_pixels = 0usize;
    let mut has_transparency = false;

    for pixel in image.pixels() {
        let alpha = pixel.0[3];
        if alpha == 0 {
            has_transparency = true;
            continue;
        }
        if alpha < 255 {
            has_transparency = true;
            partial_alpha_pixels += 1;
        }
        opaque_pixels += 1;
    }

    // Count colors the way the pipeline will see them: built from interior
    // pixels and merged at the logo threshold. Counting raw distinct RGBA
    // values would just count anti-aliasing.
    let probe = build_palette(image, 32, 196);
    let distinct_colors = probe
        .opaque_indices()
        .count()
        .clamp(1, u8::MAX as usize) as u8;

    ImageStats {
        opaque_pixels,
        partial_alpha_pixels,
        distinct_colors,
        has_transparency,
    }
}

fn infer_mode(image: &RgbaImage, stats: &ImageStats) -> VectorizeMode {
    let max_dimension = image.width().max(image.height());

    // Small, hard-edged, few colors: treat as pixel art and keep every edge
    // exactly where it is.
    if max_dimension <= 16 && stats.distinct_colors <= 16 && stats.partial_alpha_pixels == 0 {
        return VectorizeMode::PixelArt;
    }

    if !stats.has_transparency && (stats.distinct_colors > 24 || stats.opaque_pixels > 80_000) {
        return VectorizeMode::Poster;
    }

    VectorizeMode::Logo
}

/// Measure how well a finished document reproduces the source image.
///
/// Exposed because it is the only honest way to compare two vectorizers: render
/// both back and see which one matches the input.
pub fn accuracy(document: &Document, image: &RgbaImage) -> f64 {
    let width = image.width() as usize;
    let height = image.height() as usize;
    if width == 0 || height == 0 {
        return 0.0;
    }

    let palette = build_palette(image, 32, 0);
    let decomposition = decompose(image, &palette);

    // Accumulate the document's own coverage per palette color, then compare
    // against what the image showed.
    let mut rendered: Vec<Field> = (0..palette.len())
        .map(|_| Field::new(width, height))
        .collect();

    for shape in &document.shapes {
        let entry = nearest_palette_entry(&palette, shape.color);
        let mask = rasterize(&shape.contours(), (0, 0, width, height));
        let field = &mut rendered[entry];
        for index in 0..field.data.len() {
            field.data[index] = (field.data[index] + mask.data[index]).min(1.0);
        }
    }

    // Anything the document did not paint reads as transparent. Without this the
    // score would penalise every uncovered pixel of the canvas, since no shape
    // is ever emitted for the transparent palette entry.
    if let Some(transparent) = palette.transparent {
        for index in 0..width * height {
            let painted: f32 = rendered
                .iter()
                .enumerate()
                .filter(|(entry, _)| *entry != transparent)
                .map(|(_, field)| field.data[index])
                .sum();
            rendered[transparent].data[index] = (1.0 - painted).max(0.0);
        }
    }

    let mut total_error = 0.0f64;
    for (measured, drawn) in decomposition.coverage.iter().zip(rendered.iter()) {
        for (&expected, &actual) in measured.data.iter().zip(drawn.data.iter()) {
            total_error += (expected as f64 - actual as f64).abs();
        }
    }

    // Halve because a misplaced pixel is counted once in the color it should
    // have been and once in the color it became.
    let mean_error = total_error / (2.0 * (width * height) as f64);
    (1.0 - mean_error).clamp(0.0, 1.0)
}

/// Nearest *opaque* palette entry. A shape always has a visible fill, so the
/// transparent entry must never win the match.
fn nearest_palette_entry(palette: &crate::quantize::Palette, color: [u8; 4]) -> usize {
    let mut best = 0usize;
    let mut best_distance = u32::MAX;
    for index in palette.opaque_indices() {
        let distance = crate::quantize::perceptual_distance_sq(palette.colors[index], color);
        if distance < best_distance {
            best_distance = distance;
            best = index;
        }
    }
    best
}
