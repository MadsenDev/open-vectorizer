use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as FmtWrite;

use image::RgbaImage;
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[cfg(target_arch = "wasm32")]
use wasm_bindgen::prelude::*;

#[derive(Debug, Error)]
pub enum VectorizeError {
    #[error("failed to decode image: {0}")]
    Decode(#[from] image::ImageError),
    #[error("vectorization failed: {0}")]
    Vectorize(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VectorizeMode {
    Auto,
    Logo,
    Poster,
    #[serde(rename = "pixel", alias = "pixelart", alias = "pixel-art")]
    PixelArt,
}

impl Default for VectorizeMode {
    fn default() -> Self {
        Self::Auto
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VectorizeOptions {
    pub colors: u8,
    pub detail: f32,
    pub smoothness: f32,
    pub tolerance: f32,
    pub mode: VectorizeMode,
}

impl Default for VectorizeOptions {
    fn default() -> Self {
        Self {
            colors: 8,
            detail: 0.6,
            smoothness: 0.5,
            tolerance: 1.5,
            mode: VectorizeMode::Auto,
        }
    }
}

pub fn png_to_svg(png_bytes: &[u8], options: &VectorizeOptions) -> Result<String, VectorizeError> {
    let image = image::load_from_memory(png_bytes)?;
    let rgba = image.to_rgba8();
    let effective_options = resolve_auto_options(&rgba, options);

    let quantized = quantize_image(&rgba, &effective_options);
    let svg = render_svg(&quantized, &effective_options);

    Ok(svg)
}

fn resolve_auto_options(image: &RgbaImage, options: &VectorizeOptions) -> VectorizeOptions {
    if options.mode != VectorizeMode::Auto {
        return options.clone();
    }

    let stats = analyze_image(image);
    let mode = infer_mode(image, &stats);
    match mode {
        VectorizeMode::PixelArt => VectorizeOptions {
            colors: stats.unique_opaque_colors.clamp(2, 16),
            detail: 1.0,
            smoothness: 0.0,
            tolerance: 0.5,
            mode,
        },
        VectorizeMode::Poster => VectorizeOptions {
            colors: stats.unique_opaque_colors.clamp(8, 32),
            detail: 0.85,
            smoothness: 0.45,
            tolerance: 2.25,
            mode,
        },
        _ => VectorizeOptions {
            colors: stats.unique_opaque_colors.clamp(2, 8),
            detail: 0.65,
            smoothness: 0.72,
            tolerance: 2.0,
            mode: VectorizeMode::Logo,
        },
    }
}

#[derive(Debug)]
struct ImageStats {
    opaque_pixels: usize,
    partial_alpha_pixels: usize,
    unique_opaque_colors: u8,
    has_transparency: bool,
}

fn analyze_image(image: &RgbaImage) -> ImageStats {
    let mut unique = HashSet::new();
    let mut opaque_pixels = 0usize;
    let mut partial_alpha_pixels = 0usize;
    let mut has_transparency = false;

    for pixel in image.pixels() {
        if pixel[3] == 0 {
            has_transparency = true;
            continue;
        }
        if pixel[3] < 255 {
            has_transparency = true;
            partial_alpha_pixels += 1;
        }
        opaque_pixels += 1;
        if unique.len() <= 64 {
            unique.insert([pixel[0], pixel[1], pixel[2], pixel[3]]);
        }
    }

    ImageStats {
        opaque_pixels,
        partial_alpha_pixels,
        unique_opaque_colors: unique.len().clamp(0, u8::MAX as usize) as u8,
        has_transparency,
    }
}

fn infer_mode(image: &RgbaImage, stats: &ImageStats) -> VectorizeMode {
    let max_dimension = image.width().max(image.height());
    if max_dimension <= 16
        && stats.unique_opaque_colors <= 16
        && stats.partial_alpha_pixels == 0
    {
        return VectorizeMode::PixelArt;
    }

    if !stats.has_transparency
        && (stats.unique_opaque_colors > 24 || stats.opaque_pixels > 80_000)
    {
        return VectorizeMode::Poster;
    }

    VectorizeMode::Logo
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn png_to_svg_wasm(png_bytes: &[u8], options_json: &str) -> Result<String, JsValue> {
    let options = if options_json.trim().is_empty() {
        VectorizeOptions::default()
    } else {
        serde_json::from_str::<VectorizeOptions>(options_json)
            .map_err(|err| JsValue::from_str(&format!("invalid options json: {err}")))?
    };

    png_to_svg(png_bytes, &options).map_err(|err| JsValue::from_str(&err.to_string()))
}

#[cfg(target_arch = "wasm32")]
#[wasm_bindgen]
pub fn default_options_json() -> String {
    serde_json::to_string(&VectorizeOptions::default()).unwrap_or_else(|_| "{}".to_string())
}

fn palette_size_from_options(options: &VectorizeOptions) -> usize {
    let clamped_detail = options.detail.clamp(0.1, 1.0);
    let base = options.colors.max(2) as f32;
    (base * clamped_detail).ceil() as usize
}

#[derive(Debug, Clone)]
struct QuantizedImage {
    palette: Vec<[u8; 4]>,
    indices: Vec<usize>,
    width: u32,
    height: u32,
}

fn quantize_image(image: &RgbaImage, options: &VectorizeOptions) -> QuantizedImage {
    let palette_size = palette_size_from_options(options);
    
    // Check if image has transparent pixels
    let has_transparency = image.pixels().any(|p| p[3] < 255);
    
    // Reserve one slot for transparent if needed, otherwise use full palette_size
    let opaque_palette_size = if has_transparency {
        palette_size.saturating_sub(1)
    } else {
        palette_size
    };
    
    let mut palette = build_palette(image, opaque_palette_size.max(1));
    palette = merge_similar_palette_colors(palette, options);
    
    // Add transparent color to palette if image has transparency
    if has_transparency {
        palette.push([0, 0, 0, 0]);
    }
    
    let indices = map_to_palette(image, &palette);

    QuantizedImage {
        palette,
        indices,
        width: image.width(),
        height: image.height(),
    }
}

fn build_palette(image: &RgbaImage, max_colors: usize) -> Vec<[u8; 4]> {
    // Collect all non-transparent pixels
    let mut pixels: Vec<[u8; 4]> = Vec::new();
    for pixel in image.pixels() {
        if pixel[3] > 0 {
            pixels.push(pixel.0);
        }
    }

    if pixels.is_empty() {
        return vec![[0, 0, 0, 0]];
    }

    if pixels.len() <= max_colors {
        // If we have fewer unique pixels than max_colors, just return unique colors
        let mut unique: Vec<[u8; 4]> = pixels.into_iter().collect::<std::collections::HashSet<_>>().into_iter().collect();
        if unique.is_empty() {
            unique.push([0, 0, 0, 0]);
        }
        return unique;
    }

    // Use median cut algorithm for better color distribution
    median_cut_quantize(&pixels, max_colors.max(1))
}

#[derive(Clone)]
struct ColorBox {
    pixels: Vec<[u8; 4]>,
    r_min: u8,
    r_max: u8,
    g_min: u8,
    g_max: u8,
    b_min: u8,
    b_max: u8,
}

impl ColorBox {
    fn new(pixels: Vec<[u8; 4]>) -> Self {
        if pixels.is_empty() {
            return Self {
                pixels,
                r_min: 0,
                r_max: 0,
                g_min: 0,
                g_max: 0,
                b_min: 0,
                b_max: 0,
            };
        }

        let mut r_min = 255u8;
        let mut r_max = 0u8;
        let mut g_min = 255u8;
        let mut g_max = 0u8;
        let mut b_min = 255u8;
        let mut b_max = 0u8;

        for &[r, g, b, _] in &pixels {
            r_min = r_min.min(r);
            r_max = r_max.max(r);
            g_min = g_min.min(g);
            g_max = g_max.max(g);
            b_min = b_min.min(b);
            b_max = b_max.max(b);
        }

        Self {
            pixels,
            r_min,
            r_max,
            g_min,
            g_max,
            b_min,
            b_max,
        }
    }

    fn longest_dimension(&self) -> usize {
        let r_range = (self.r_max as i32 - self.r_min as i32) as u32;
        let g_range = (self.g_max as i32 - self.g_min as i32) as u32;
        let b_range = (self.b_max as i32 - self.b_min as i32) as u32;

        if r_range >= g_range && r_range >= b_range {
            0 // R
        } else if g_range >= b_range {
            1 // G
        } else {
            2 // B
        }
    }

    fn average_color(&self) -> [u8; 4] {
        if self.pixels.is_empty() {
            return [0, 0, 0, 0];
        }

        let mut r_sum = 0u32;
        let mut g_sum = 0u32;
        let mut b_sum = 0u32;
        let mut a_sum = 0u32;

        for &[r, g, b, a] in &self.pixels {
            r_sum += r as u32;
            g_sum += g as u32;
            b_sum += b as u32;
            a_sum += a as u32;
        }

        let count = self.pixels.len() as u32;
        [
            (r_sum / count) as u8,
            (g_sum / count) as u8,
            (b_sum / count) as u8,
            (a_sum / count) as u8,
        ]
    }
}

fn median_cut_quantize(pixels: &[[u8; 4]], max_colors: usize) -> Vec<[u8; 4]> {
    if pixels.is_empty() {
        return vec![[0, 0, 0, 0]];
    }

    let mut boxes = vec![ColorBox::new(pixels.to_vec())];

    while boxes.len() < max_colors {
        // Find the box with the most pixels that can be split
        let box_idx = boxes
            .iter()
            .enumerate()
            .filter(|(_, b)| b.pixels.len() > 1)
            .max_by_key(|(_, b)| b.pixels.len())
            .map(|(i, _)| i);

        let box_idx = match box_idx {
            Some(idx) => idx,
            None => {
                // No more boxes can be split, break early
                break;
            }
        };

        let box_to_split = boxes.remove(box_idx);

        let dim = box_to_split.longest_dimension();

        // Sort pixels by the longest dimension
        let mut sorted_pixels = box_to_split.pixels;
        sorted_pixels.sort_by_key(|pixel| pixel[dim]);

        // Split at median
        let median = sorted_pixels.len() / 2;
        let (left_pixels, right_pixels) = sorted_pixels.split_at(median);

        // Only add boxes if they have pixels
        if !left_pixels.is_empty() {
            boxes.push(ColorBox::new(left_pixels.to_vec()));
        }
        if !right_pixels.is_empty() {
            boxes.push(ColorBox::new(right_pixels.to_vec()));
        }

        // If we couldn't split, we're done
        if boxes.len() == 1 && boxes[0].pixels.len() <= 1 {
            break;
        }
    }

    // Return average colors from each box
    let mut palette: Vec<[u8; 4]> = boxes.iter().map(|b| b.average_color()).collect();
    
    // If we have fewer colors than requested and there are still unique colors, try to add more
    if palette.len() < max_colors && !pixels.is_empty() {
        // Collect unique colors from pixels
        let unique_colors: std::collections::HashSet<[u8; 4]> = pixels.iter().copied().collect();
        if unique_colors.len() > palette.len() {
            // Add unique colors that aren't already in palette
            for &color in &unique_colors {
                if palette.len() >= max_colors {
                    break;
                }
                // Check if color is similar to any in palette
                let is_similar = palette.iter().any(|&pal_color| {
                    color_distance(color, pal_color) < 100 // Threshold for "similar"
                });
                if !is_similar {
                    palette.push(color);
                }
            }
        }
    }

    palette
}

fn map_to_palette(image: &RgbaImage, palette: &[[u8; 4]]) -> Vec<usize> {
    // Find transparent color index (should be last if present)
    let transparent_idx = palette.iter().position(|&c| c[3] == 0);
    
    // Build separate palettes for opaque and transparent
    let opaque_palette: Vec<(usize, [u8; 4])> = palette
        .iter()
        .enumerate()
        .filter(|(_, c)| c[3] > 0)
        .map(|(idx, &c)| (idx, c))
        .collect();
    
    image
        .pixels()
        .map(|pixel| {
            // If pixel is transparent, map to transparent palette entry
            if pixel[3] == 0 {
                transparent_idx.unwrap_or(0)
            } else if opaque_palette.is_empty() {
                0
            } else {
                // Find nearest opaque color
                let mut best_idx = 0;
                let mut best_dist = u32::MAX;
                for &(orig_idx, color) in &opaque_palette {
                    let dist = color_distance(pixel.0, color);
                    if dist < best_dist {
                        best_idx = orig_idx;
                        best_dist = dist;
                    }
                }
                best_idx
            }
        })
        .collect()
}

fn color_distance(a: [u8; 4], b: [u8; 4]) -> u32 {
    let dr = a[0] as i32 - b[0] as i32;
    let dg = a[1] as i32 - b[1] as i32;
    let db = a[2] as i32 - b[2] as i32;
    let da = a[3] as i32 - b[3] as i32;
    (dr * dr + dg * dg + db * db + da * da) as u32
}

fn merge_similar_palette_colors(
    palette: Vec<[u8; 4]>,
    options: &VectorizeOptions,
) -> Vec<[u8; 4]> {
    let threshold = palette_merge_threshold(options);
    if threshold == 0 {
        return palette;
    }

    let mut merged: Vec<[u8; 4]> = Vec::new();
    for color in palette {
        if color[3] == 0 {
            merged.push(color);
            continue;
        }

        if let Some(existing) = merged
            .iter_mut()
            .find(|existing| existing[3] > 0 && color_distance(**existing, color) <= threshold)
        {
            *existing = average_pair(*existing, color);
        } else {
            merged.push(color);
        }
    }

    merged
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

fn average_pair(a: [u8; 4], b: [u8; 4]) -> [u8; 4] {
    [
        ((a[0] as u16 + b[0] as u16) / 2) as u8,
        ((a[1] as u16 + b[1] as u16) / 2) as u8,
        ((a[2] as u16 + b[2] as u16) / 2) as u8,
        ((a[3] as u16 + b[3] as u16) / 2) as u8,
    ]
}

fn render_svg(quantized: &QuantizedImage, options: &VectorizeOptions) -> String {
    let mut svg = String::with_capacity(quantized.width as usize * quantized.height as usize / 10);
    writeln!(
        svg,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" aria-label=\"vectorized\">",
        w = quantized.width,
        h = quantized.height
    )
    .ok();

    // Group paths by color
    let mut paths_by_color: BTreeMap<usize, Vec<String>> = BTreeMap::new();

    // For each color, find connected components and trace contours
    for (color_idx, &color) in quantized.palette.iter().enumerate() {
        if color[3] == 0 {
            continue; // Skip transparent
        }

        let components = find_connected_components(quantized, color_idx);

        for component in components {
            if component.len() < minimum_component_area(quantized, options) {
                continue;
            }

            let contours = trace_contours(&component);
            let path_d = contours_to_path(&contours, options);
            if !path_d.is_empty() {
                paths_by_color
                    .entry(color_idx)
                    .or_insert_with(Vec::new)
                    .push(path_d);
            }
        }
    }

    // Output paths grouped by color
    for (color_idx, paths) in paths_by_color {
        let color = quantized.palette[color_idx];
        let opacity = opacity_from_options(color[3], options);
        let hex = to_hex(color);
        
        writeln!(
            svg,
            "  <g fill=\"#{hex}\" fill-opacity=\"{opacity:.3}\">",
            hex = hex,
            opacity = opacity
        )
        .ok();
        
        for path_d in paths {
            writeln!(svg, "    <path fill-rule=\"evenodd\" d=\"{}\"/>", path_d).ok();
        }
        
        writeln!(svg, "  </g>").ok();
    }

    svg.push_str("</svg>");
    svg
}

fn minimum_component_area(quantized: &QuantizedImage, options: &VectorizeOptions) -> usize {
    if quantized.width * quantized.height <= 16 {
        return 1;
    }

    match options.mode {
        VectorizeMode::PixelArt => 1,
        VectorizeMode::Auto | VectorizeMode::Logo => {
            if options.detail >= 0.85 {
                1
            } else if options.detail >= 0.55 {
                2
            } else {
                4
            }
        }
        VectorizeMode::Poster => {
            if options.detail >= 0.8 {
                2
            } else {
                4
            }
        }
    }
}

// Point type for contours with sub-pixel precision
#[derive(Debug, Clone, Copy, PartialEq)]
struct Point {
    x: f32,
    y: f32,
}

impl Point {
    fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

impl std::hash::Hash for Point {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        (self.x as i32).hash(state);
        (self.y as i32).hash(state);
    }
}

impl Eq for Point {}

// Find connected components using 8-connectivity
fn find_connected_components(quantized: &QuantizedImage, color_idx: usize) -> Vec<HashSet<(i32, i32)>> {
    let width = quantized.width as usize;
    let height = quantized.height as usize;
    let mut visited = HashSet::new();
    let mut components = Vec::new();

    for y in 0..height {
        for x in 0..width {
            let idx = y * width + x;
            if quantized.indices[idx] == color_idx {
                let point = (x as i32, y as i32);
                if !visited.contains(&point) {
                    // Flood fill to find connected component
                    let mut component = HashSet::new();
                    let mut stack = vec![point];
                    visited.insert(point);

                    while let Some((px, py)) = stack.pop() {
                        component.insert((px, py));

                        // Check 8 neighbors
                        for dy in -1..=1 {
                            for dx in -1..=1 {
                                if dx == 0 && dy == 0 {
                                    continue;
                                }
                                let nx = px + dx;
                                let ny = py + dy;
                                if nx >= 0
                                    && ny >= 0
                                    && nx < width as i32
                                    && ny < height as i32
                                {
                                    let nidx = (ny as usize) * width + (nx as usize);
                                    if quantized.indices[nidx] == color_idx {
                                        let neighbor = (nx, ny);
                                        if !visited.contains(&neighbor) {
                                            visited.insert(neighbor);
                                            stack.push(neighbor);
                                        }
                                    }
                                }
                            }
                        }
                    }

                    if !component.is_empty() {
                        components.push(component);
                    }
                }
            }
        }
    }

    components
}

type GridPoint = (i32, i32);

// Trace exact cell-edge contours for a connected component. Unlike boundary-pixel
// walking, this follows the edges between filled and empty cells, so the resulting
// path describes the actual raster silhouette and can include inner holes.
fn trace_contours(component: &HashSet<(i32, i32)>) -> Vec<Vec<Point>> {
    if component.is_empty() {
        return Vec::new();
    }

    let mut edges_by_start: BTreeMap<GridPoint, Vec<GridPoint>> = BTreeMap::new();
    for &(x, y) in component {
        if !component.contains(&(x, y - 1)) {
            push_edge(&mut edges_by_start, (x, y), (x + 1, y));
        }
        if !component.contains(&(x + 1, y)) {
            push_edge(&mut edges_by_start, (x + 1, y), (x + 1, y + 1));
        }
        if !component.contains(&(x, y + 1)) {
            push_edge(&mut edges_by_start, (x + 1, y + 1), (x, y + 1));
        }
        if !component.contains(&(x - 1, y)) {
            push_edge(&mut edges_by_start, (x, y + 1), (x, y));
        }
    }

    let mut contours = Vec::new();
    while let Some(start) = first_start_with_edges(&edges_by_start) {
        let mut contour = vec![grid_to_point(start)];
        let mut current = start;
        let mut guard = 0usize;

        while let Some(next) = pop_edge(&mut edges_by_start, current) {
            contour.push(grid_to_point(next));
            current = next;
            guard += 1;

            if current == start {
                break;
            }
            if guard > component.len() * 8 + 8 {
                break;
            }
        }

        if contour.len() >= 4 && contour.first() == contour.last() {
            contours.push(contour);
        }
    }

    contours
}

fn push_edge(
    edges_by_start: &mut BTreeMap<GridPoint, Vec<GridPoint>>,
    start: GridPoint,
    end: GridPoint,
) {
    edges_by_start.entry(start).or_default().push(end);
}

fn first_start_with_edges(
    edges_by_start: &BTreeMap<GridPoint, Vec<GridPoint>>,
) -> Option<GridPoint> {
    edges_by_start
        .iter()
        .find_map(|(&start, ends)| if ends.is_empty() { None } else { Some(start) })
}

fn pop_edge(
    edges_by_start: &mut BTreeMap<GridPoint, Vec<GridPoint>>,
    start: GridPoint,
) -> Option<GridPoint> {
    let ends = edges_by_start.get_mut(&start)?;
    let next = ends.pop();
    let should_remove = ends.is_empty();
    if should_remove {
        edges_by_start.remove(&start);
    }
    next
}

fn grid_to_point(point: GridPoint) -> Point {
    Point::new(point.0 as f32, point.1 as f32)
}

// Ramer-Douglas-Peucker path simplification
fn rdp_simplify(points: &[Point], tolerance: f32) -> Vec<Point> {
    if points.len() <= 2 {
        return points.to_vec();
    }

    let tol_sq = tolerance * tolerance;

    // Find the point with maximum distance from line between first and last
    let mut max_dist_sq = 0.0;
    let mut max_idx = 0;

    let p1 = points[0];
    let p2 = points[points.len() - 1];

    for (i, &p) in points.iter().enumerate().skip(1).take(points.len() - 2) {
        let dist_sq = point_to_line_dist_sq(p, p1, p2);
        if dist_sq > max_dist_sq {
            max_dist_sq = dist_sq;
            max_idx = i;
        }
    }

    // If max distance is greater than tolerance, recursively simplify
    if max_dist_sq > tol_sq {
        let mut result = rdp_simplify(&points[..=max_idx], tolerance);
        result.pop(); // Remove duplicate point
        result.extend_from_slice(&rdp_simplify(&points[max_idx..], tolerance));
        result
    } else {
        // Return just the endpoints
        vec![points[0], points[points.len() - 1]]
    }
}

fn point_to_line_dist_sq(p: Point, line_p1: Point, line_p2: Point) -> f32 {
    let dx = (line_p2.x - line_p1.x) as f32;
    let dy = (line_p2.y - line_p1.y) as f32;
    let len_sq = dx * dx + dy * dy;

    if len_sq < 1e-6 {
        // Line segment is a point
        let px = (p.x - line_p1.x) as f32;
        let py = (p.y - line_p1.y) as f32;
        return px * px + py * py;
    }

    let t = ((p.x - line_p1.x) as f32 * dx + (p.y - line_p1.y) as f32 * dy) / len_sq;
    let t = t.clamp(0.0, 1.0);

    let proj_x = line_p1.x as f32 + t * dx;
    let proj_y = line_p1.y as f32 + t * dy;

    let px = p.x as f32 - proj_x;
    let py = p.y as f32 - proj_y;

    px * px + py * py
}

fn contours_to_path(contours: &[Vec<Point>], options: &VectorizeOptions) -> String {
    let mut path = String::new();
    for contour in contours {
        let simplified = simplify_contour(contour, options);
        let subpath = points_to_subpath(&simplified, options);
        if !subpath.is_empty() {
            if !path.is_empty() {
                path.push(' ');
            }
            path.push_str(&subpath);
        }
    }
    path
}

fn simplify_contour(contour: &[Point], options: &VectorizeOptions) -> Vec<Point> {
    let tolerance = match options.mode {
        VectorizeMode::Auto | VectorizeMode::Logo => {
            (options.tolerance * (1.15 - options.detail.clamp(0.1, 1.0) * 0.45))
                .clamp(0.35, 2.5)
        }
        VectorizeMode::Poster => (options.tolerance * 0.75).clamp(0.3, 3.0),
        VectorizeMode::PixelArt => 0.0,
    };

    if tolerance <= 0.0 {
        contour.to_vec()
    } else {
        let simplified = simplify_closed_contour(contour, tolerance);
        if simplified.len() >= 4 {
            simplified
        } else {
            contour.to_vec()
        }
    }
}

fn simplify_closed_contour(contour: &[Point], tolerance: f32) -> Vec<Point> {
    if contour.len() <= 4 || contour.first() != contour.last() {
        return rdp_simplify(contour, tolerance);
    }

    let open = &contour[..contour.len() - 1];
    let split = farthest_point_pair(open);
    let mut first_arc = rdp_simplify(&open[split.0..=split.1], tolerance);

    let mut second_arc = Vec::with_capacity(open.len() - (split.1 - split.0) + 1);
    second_arc.extend_from_slice(&open[split.1..]);
    second_arc.extend_from_slice(&open[..=split.0]);
    let mut second_arc = rdp_simplify(&second_arc, tolerance);

    first_arc.pop();
    second_arc.pop();
    first_arc.extend(second_arc);
    first_arc.push(first_arc[0]);
    first_arc
}

fn farthest_point_pair(points: &[Point]) -> (usize, usize) {
    let mut pair = (0, points.len() / 2);
    let mut max_dist_sq = 0.0;

    for (i, &a) in points.iter().enumerate() {
        for (j, &b) in points.iter().enumerate().skip(i + 1) {
            let dx = a.x - b.x;
            let dy = a.y - b.y;
            let dist_sq = dx * dx + dy * dy;
            if dist_sq > max_dist_sq {
                max_dist_sq = dist_sq;
                pair = (i, j);
            }
        }
    }

    pair
}

// Convert one closed contour to an SVG subpath.
fn points_to_subpath(points: &[Point], options: &VectorizeOptions) -> String {
    if points.len() < 2 {
        return String::new();
    }

    if matches!(options.mode, VectorizeMode::Auto | VectorizeMode::Logo) {
        if let Some(circle) = fit_circle(points) {
            return circle_to_subpath(circle);
        }
    }

    let mut path = String::new();
    let smoothness = options.smoothness.clamp(0.0, 1.0);

    // Start path
    write!(path, "M {:.2} {:.2}", points[0].x, points[0].y).ok();
    
    if matches!(options.mode, VectorizeMode::Auto | VectorizeMode::Logo)
        && smoothness >= 0.35
        && points.len() > 4
    {
        write_closed_bezier_path(&mut path, points, smoothness);
    } else {
        // Simple polyline for accuracy
        for p in points.iter().skip(1) {
            write!(path, " L {:.2} {:.2}", p.x, p.y).ok();
        }
    }
    
    path.push_str(" Z");
    path
}

#[derive(Debug, Clone, Copy)]
struct Circle {
    center: Point,
    radius: f32,
}

fn fit_circle(points: &[Point]) -> Option<Circle> {
    let ring = closed_ring_points(points)?;
    if ring.len() < 12 {
        return None;
    }

    let (min_x, max_x, min_y, max_y) = bounds(ring);
    let width = max_x - min_x;
    let height = max_y - min_y;
    if width < 6.0 || height < 6.0 {
        return None;
    }

    let aspect_error = (width - height).abs() / width.max(height);
    if aspect_error > 0.08 {
        return None;
    }

    let center = Point::new((min_x + max_x) / 2.0, (min_y + max_y) / 2.0);
    let expected_radius = (width + height) / 4.0;
    let mut max_error = 0.0f32;
    let mut total_error = 0.0f32;

    for &point in ring {
        let radius = distance_between(point, center);
        let error = (radius - expected_radius).abs();
        max_error = max_error.max(error);
        total_error += error;
    }

    let mean_error = total_error / ring.len() as f32;
    if mean_error / expected_radius > 0.09 || max_error / expected_radius > 0.22 {
        return None;
    }

    Some(Circle {
        center,
        radius: expected_radius,
    })
}

fn closed_ring_points(points: &[Point]) -> Option<&[Point]> {
    if points.first() == points.last() && points.len() > 1 {
        Some(&points[..points.len() - 1])
    } else {
        None
    }
}

fn bounds(points: &[Point]) -> (f32, f32, f32, f32) {
    let mut min_x = f32::INFINITY;
    let mut max_x = f32::NEG_INFINITY;
    let mut min_y = f32::INFINITY;
    let mut max_y = f32::NEG_INFINITY;

    for &point in points {
        min_x = min_x.min(point.x);
        max_x = max_x.max(point.x);
        min_y = min_y.min(point.y);
        max_y = max_y.max(point.y);
    }

    (min_x, max_x, min_y, max_y)
}

fn distance_between(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}

fn circle_to_subpath(circle: Circle) -> String {
    let cx = circle.center.x;
    let cy = circle.center.y;
    let r = circle.radius;
    format!(
        "M {:.2} {:.2} A {:.2} {:.2} 0 1 0 {:.2} {:.2} A {:.2} {:.2} 0 1 0 {:.2} {:.2} Z",
        cx + r,
        cy,
        r,
        r,
        cx - r,
        cy,
        r,
        r,
        cx + r,
        cy
    )
}

fn write_closed_bezier_path(path: &mut String, points: &[Point], smoothness: f32) {
    let ring = if points.first() == points.last() {
        &points[..points.len() - 1]
    } else {
        points
    };
    if ring.len() < 4 {
        for p in points.iter().skip(1) {
            write!(path, " L {:.2} {:.2}", p.x, p.y).ok();
        }
        return;
    }

    let tension = smoothness * 0.95;
    for i in 0..ring.len() {
        let p0 = ring[(i + ring.len() - 1) % ring.len()];
        let p1 = ring[i];
        let p2 = ring[(i + 1) % ring.len()];
        let p3 = ring[(i + 2) % ring.len()];

        let cp1x = p1.x + (p2.x - p0.x) * tension / 6.0;
        let cp1y = p1.y + (p2.y - p0.y) * tension / 6.0;
        let cp2x = p2.x - (p3.x - p1.x) * tension / 6.0;
        let cp2y = p2.y - (p3.y - p1.y) * tension / 6.0;

        write!(
            path,
            " C {:.2} {:.2} {:.2} {:.2} {:.2} {:.2}",
            cp1x, cp1y, cp2x, cp2y, p2.x, p2.y
        )
        .ok();
    }
}

fn opacity_from_options(alpha: u8, _options: &VectorizeOptions) -> f32 {
    alpha as f32 / 255.0
}

fn to_hex(color: [u8; 4]) -> String {
    let mut s = String::with_capacity(6);
    write!(&mut s, "{:02x}{:02x}{:02x}", color[0], color[1], color[2]).ok();
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::png::PngEncoder, ColorType, DynamicImage, ImageEncoder, Rgba};
    use serde_json::json;
    use std::collections::HashSet;

    #[test]
    fn creates_svg_output() {
        let image = RgbaImage::from_fn(2, 2, |x, y| {
            let alpha = if (x + y) % 2 == 0 { 255 } else { 128 };
            Rgba([x as u8 * 80, y as u8 * 40, 200, alpha])
        });

        let mut png_bytes = Vec::new();
        PngEncoder::new(&mut png_bytes)
            .write_image(
                image.as_raw(),
                image.width(),
                image.height(),
                ColorType::Rgba8.into(),
            )
            .expect("image should encode to png");

        let options = VectorizeOptions::default();
        let svg = png_to_svg(&png_bytes, &options).expect("svg generation should succeed");

        assert!(svg.contains("<svg"));
        // Check that SVG has some content (path, rect, or group)
        assert!(
            svg.contains("path") || svg.contains("rect") || svg.contains("<g>"),
            "SVG should contain path, rect, or group. Got: {}",
            svg
        );
    }

    #[test]
    fn respects_palette_size() {
        let image = DynamicImage::new_rgba8(4, 4).to_rgba8();
        let palette = build_palette(&image, 4);
        assert_eq!(palette.len(), 1, "empty images fall back to one color");

        let non_empty = RgbaImage::from_fn(4, 4, |x, y| {
            let alpha = if (x + y) % 2 == 0 { 255 } else { 128 };
            Rgba([x as u8 * 10, y as u8 * 10, 50, alpha])
        });
        let palette = build_palette(&non_empty, 3);
        assert!(palette.len() <= 3);
    }

    #[test]
    fn options_round_trip_json() {
        let json = json!({
            "colors": 12,
            "detail": 0.75,
            "smoothness": 0.4,
            "tolerance": 2.0,
            "mode": "pixel",
        });

        let options: VectorizeOptions =
            serde_json::from_value(json).expect("options should deserialize");
        assert_eq!(options.colors, 12);
        assert_eq!(options.detail, 0.75);
        assert_eq!(options.smoothness, 0.4);
        assert_eq!(options.tolerance, 2.0);
        assert!(matches!(options.mode, VectorizeMode::PixelArt));

        let serialized = serde_json::to_string(&options).expect("options should serialize");
        assert!(serialized.contains("\"mode\":\"pixel\""));
    }

    #[test]
    fn default_options_start_in_auto_mode() {
        assert!(matches!(VectorizeOptions::default().mode, VectorizeMode::Auto));
    }

    #[test]
    fn auto_mode_infers_logo_for_transparent_mark() {
        let image = RgbaImage::from_fn(32, 32, |x, y| {
            let dx = x as i32 - 16;
            let dy = y as i32 - 16;
            let dist_sq = dx * dx + dy * dy;
            if (70..=190).contains(&dist_sq) {
                Rgba([30, 30, 34, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });

        let options = resolve_auto_options(&image, &VectorizeOptions::default());

        assert!(matches!(options.mode, VectorizeMode::Logo));
        assert!(options.smoothness > 0.5);
        assert!(options.tolerance > VectorizeOptions::default().tolerance);
    }

    #[test]
    fn auto_mode_keeps_tiny_low_color_art_crisp() {
        let image = RgbaImage::from_fn(12, 12, |x, y| {
            if x == y || x + 1 == y {
                Rgba([20, 20, 20, 255])
            } else {
                Rgba([0, 0, 0, 0])
            }
        });

        let options = resolve_auto_options(&image, &VectorizeOptions::default());

        assert!(matches!(options.mode, VectorizeMode::PixelArt));
        assert_eq!(options.smoothness, 0.0);
    }

    #[test]
    fn quantize_image_tracks_dimensions() {
        let image = RgbaImage::from_fn(3, 2, |x, y| {
            let alpha = if x == 0 { 0 } else { 255 };
            Rgba([x as u8 * 20, y as u8 * 30, 10, alpha])
        });

        let options = VectorizeOptions::default();
        let quantized = quantize_image(&image, &options);

        assert_eq!(quantized.width, 3);
        assert_eq!(quantized.height, 2);
        assert_eq!(quantized.indices.len(), 6);
        assert!(!quantized.palette.is_empty());
    }

    #[test]
    fn traces_single_pixel_as_cell_edges() {
        let component = HashSet::from([(2, 3)]);
        let contours = trace_contours(&component);

        assert_eq!(contours.len(), 1);
        assert_eq!(
            contours[0],
            vec![
                Point::new(2.0, 3.0),
                Point::new(3.0, 3.0),
                Point::new(3.0, 4.0),
                Point::new(2.0, 4.0),
                Point::new(2.0, 3.0),
            ]
        );
    }

    #[test]
    fn traces_inner_holes_as_extra_contours() {
        let component = HashSet::from([
            (0, 0),
            (1, 0),
            (2, 0),
            (0, 1),
            (2, 1),
            (0, 2),
            (1, 2),
            (2, 2),
        ]);
        let contours = trace_contours(&component);

        assert_eq!(contours.len(), 2);
        assert!(contours.iter().all(|contour| contour.first() == contour.last()));

        let options = VectorizeOptions {
            mode: VectorizeMode::PixelArt,
            ..VectorizeOptions::default()
        };
        let path = contours_to_path(&contours, &options);
        assert_eq!(path.matches("M ").count(), 2);
    }

    #[test]
    fn rendered_paths_use_even_odd_fill_for_cutouts() {
        let quantized = QuantizedImage {
            palette: vec![[0, 0, 0, 255], [0, 0, 0, 0]],
            indices: vec![0, 0, 0, 0, 1, 0, 0, 0, 0],
            width: 3,
            height: 3,
        };
        let svg = render_svg(&quantized, &VectorizeOptions::default());

        assert!(svg.contains("fill-rule=\"evenodd\""));
    }

    #[test]
    fn logo_mode_filters_tiny_disconnected_components() {
        let quantized = QuantizedImage {
            palette: vec![[0, 0, 0, 255], [0, 0, 0, 0]],
            indices: vec![
                0, 0, 0, 1, 1,
                0, 0, 0, 1, 1,
                0, 0, 0, 1, 1,
                1, 1, 1, 1, 1,
                1, 1, 1, 1, 0,
            ],
            width: 5,
            height: 5,
        };

        let svg = render_svg(&quantized, &VectorizeOptions::default());

        assert_eq!(svg.matches("<path").count(), 1);
    }

    #[test]
    fn pixel_mode_preserves_tiny_disconnected_components() {
        let quantized = QuantizedImage {
            palette: vec![[0, 0, 0, 255], [0, 0, 0, 0]],
            indices: vec![
                0, 0, 0, 1, 1,
                0, 0, 0, 1, 1,
                0, 0, 0, 1, 1,
                1, 1, 1, 1, 1,
                1, 1, 1, 1, 0,
            ],
            width: 5,
            height: 5,
        };
        let options = VectorizeOptions {
            mode: VectorizeMode::PixelArt,
            ..VectorizeOptions::default()
        };
        let svg = render_svg(&quantized, &options);

        assert_eq!(svg.matches("<path").count(), 2);
    }

    #[test]
    fn logo_mode_merges_near_duplicate_palette_colors() {
        let palette = merge_similar_palette_colors(
            vec![[220, 40, 40, 255], [225, 43, 39, 255], [20, 20, 20, 255]],
            &VectorizeOptions::default(),
        );

        assert_eq!(palette.len(), 2);
    }

    #[test]
    fn pixel_mode_keeps_near_duplicate_palette_colors() {
        let palette = merge_similar_palette_colors(
            vec![[220, 40, 40, 255], [225, 43, 39, 255]],
            &VectorizeOptions {
                mode: VectorizeMode::PixelArt,
                ..VectorizeOptions::default()
            },
        );

        assert_eq!(palette.len(), 2);
    }
}
