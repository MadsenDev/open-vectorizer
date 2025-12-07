use std::fmt::Write as FmtWrite;
use std::collections::{HashMap, HashSet};

use image::{Rgba, RgbaImage};
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VectorizeMode {
    Logo,
    Poster,
    #[serde(rename = "pixel", alias = "pixelart", alias = "pixel-art")]
    PixelArt,
}

impl Default for VectorizeMode {
    fn default() -> Self {
        Self::Logo
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
            mode: VectorizeMode::Logo,
        }
    }
}

pub fn png_to_svg(png_bytes: &[u8], options: &VectorizeOptions) -> Result<String, VectorizeError> {
    let image = image::load_from_memory(png_bytes)?;
    let rgba = image.to_rgba8();

    let quantized = quantize_image(&rgba, options);
    let svg = render_svg(&quantized, options);

    Ok(svg)
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
    let mut paths_by_color: HashMap<usize, Vec<String>> = HashMap::new();

    // For each color, find connected components and trace contours
    for (color_idx, &color) in quantized.palette.iter().enumerate() {
        if color[3] == 0 {
            continue; // Skip transparent
        }

        // Find connected components for this color
        let components = find_connected_components(quantized, color_idx);
        
        for component in components {
            // Try to trace contour for this component
            // If tracing fails, create a bounding polygon to ensure all components are rendered
            if let Some(contour) = trace_contour(quantized, &component, color_idx) {
                // For logo mode, skip simplification entirely for 1-1 match
                let simplified = match options.mode {
                    VectorizeMode::Logo => contour, // No simplification - preserve every point
                    VectorizeMode::Poster => {
                        let tolerance = options.tolerance * 0.5;
                        rdp_simplify(&contour, tolerance.max(0.3))
                    },
                    VectorizeMode::PixelArt => {
                        let tolerance = options.tolerance * 2.0;
                        rdp_simplify(&contour, tolerance)
                    },
                };
                
                // Generate SVG path
                let path_d = points_to_path(&simplified, options);
                if !path_d.is_empty() {
                    paths_by_color.entry(color_idx).or_insert_with(Vec::new).push(path_d);
                }
            } else {
                // Tracing failed - create a simple bounding polygon as fallback
                // This ensures all components are rendered, even if contour tracing fails
                let min_x = component.iter().map(|p| p.0).min().unwrap_or(0);
                let max_x = component.iter().map(|p| p.0).max().unwrap_or(0);
                let min_y = component.iter().map(|p| p.1).min().unwrap_or(0);
                let max_y = component.iter().map(|p| p.1).max().unwrap_or(0);
                
                if max_x > min_x && max_y > min_y {
                    let path_d = format!("M {} {} L {} {} L {} {} L {} {} Z",
                        min_x, min_y,
                        max_x + 1, min_y,
                        max_x + 1, max_y + 1,
                        min_x, max_y + 1
                    );
                    paths_by_color.entry(color_idx).or_insert_with(Vec::new).push(path_d);
                } else if component.len() == 1 {
                    // Single pixel fallback
                    let (px, py) = component.iter().next().unwrap();
                    let path_d = format!("M {} {} L {} {} L {} {} L {} {} Z",
                        px, py,
                        px + 1, py,
                        px + 1, py + 1,
                        px, py + 1
                    );
                    paths_by_color.entry(color_idx).or_insert_with(Vec::new).push(path_d);
                }
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
            writeln!(svg, "    <path d=\"{}\"/>", path_d).ok();
        }
        
        writeln!(svg, "  </g>").ok();
    }

    svg.push_str("</svg>");
    svg
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

// Trace contour using simple, reliable boundary following
fn trace_contour(
    quantized: &QuantizedImage,
    component: &HashSet<(i32, i32)>,
    color_idx: usize,
) -> Option<Vec<Point>> {
    if component.is_empty() {
        return None;
    }

    let width = quantized.width as usize;
    let height = quantized.height as usize;

    // Build a set of boundary pixels
    let mut boundary_set = HashSet::new();
    for &(x, y) in component {
        // Check if this pixel is on the boundary
        let mut is_boundary = false;
        for dy in -1..=1 {
            for dx in -1..=1 {
                if dx == 0 && dy == 0 {
                    continue;
                }
                let nx = x + dx;
                let ny = y + dy;
                if nx < 0 || ny < 0 || nx >= width as i32 || ny >= height as i32 {
                    is_boundary = true;
                    break;
                }
                let nidx = (ny as usize) * width + (nx as usize);
                if quantized.indices[nidx] != color_idx {
                    is_boundary = true;
                    break;
                }
            }
            if is_boundary {
                break;
            }
        }
        if is_boundary {
            boundary_set.insert((x, y));
        }
    }

    if boundary_set.is_empty() {
        return None;
    }

    // Find starting point (top-leftmost)
    let start = boundary_set.iter().min_by(|a, b| a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))).copied()?;

    // Simple 8-direction neighbors
    let neighbors = [
        (-1, -1), (0, -1), (1, -1),
        (-1, 0),           (1, 0),
        (-1, 1),  (0, 1),  (1, 1),
    ];

    let mut contour = Vec::new();
    let mut current = start;
    let mut visited = HashSet::new();
    visited.insert(current);

    // Add first point
    contour.push(Point::new(current.0 as f32 + 0.5, current.1 as f32 + 0.5));

    // Follow boundary by finding connected boundary pixels
    loop {
        let mut best_next = None;
        let mut best_priority = i32::MAX;

        // Look for next boundary pixel in 8-neighborhood
        for &(dx, dy) in &neighbors {
            let nx = current.0 + dx;
            let ny = current.1 + dy;
            let candidate = (nx, ny);

            if boundary_set.contains(&candidate) && !visited.contains(&candidate) {
                // Priority: prefer 4-connected neighbors (cardinal directions) over diagonal
                // This creates smoother, more predictable paths
                let is_cardinal = dx == 0 || dy == 0;
                let priority = if is_cardinal { 0 } else { 1 };
                if priority < best_priority {
                    best_priority = priority;
                    best_next = Some(candidate);
                } else if priority == best_priority {
                    // If same priority, prefer the one we found first (maintains direction)
                    if best_next.is_none() {
                        best_next = Some(candidate);
                    }
                }
            }
        }

        if let Some(next) = best_next {
            contour.push(Point::new(next.0 as f32 + 0.5, next.1 as f32 + 0.5));
            visited.insert(next);
            current = next;

            // Check if we've closed the loop (returned to start)
            if contour.len() > 3 && current == start {
                break;
            }
            
            // Also check if we're close to the start point
            if contour.len() > 10 {
                let first = contour[0];
                let last = contour.last().unwrap();
                let dist = ((last.x - first.x).powi(2) + (last.y - first.y).powi(2)).sqrt();
                if dist < 1.5 {
                    // Close to start, add it and break
                    contour.push(contour[0]);
                    break;
                }
            }
        } else {
            // No immediate neighbor found - check if there are remaining boundary pixels
            let remaining: Vec<_> = boundary_set.iter().filter(|p| !visited.contains(p)).collect();
            if remaining.is_empty() {
                // All boundary pixels visited, close the path
                if contour.len() > 2 {
                    contour.push(contour[0]);
                }
                break;
            }
            
            // Try to find a nearby unvisited boundary pixel
            // Check in a slightly larger radius (up to 3 pixels away)
            let mut found_nearby = false;
            for radius in 2..=3 {
                for &(dx, dy) in &neighbors {
                    let check_x = current.0 + dx * radius;
                    let check_y = current.1 + dy * radius;
                    let candidate = (check_x, check_y);
                    
                    if boundary_set.contains(&candidate) && !visited.contains(&candidate) {
                        contour.push(Point::new(candidate.0 as f32 + 0.5, candidate.1 as f32 + 0.5));
                        visited.insert(candidate);
                        current = candidate;
                        found_nearby = true;
                        break;
                    }
                }
                if found_nearby {
                    break;
                }
            }
            
            if !found_nearby {
                // No nearby pixel found - this might be a separate component or the path is complete
                // Close the current path and see if we can start a new one
                if contour.len() > 2 {
                    contour.push(contour[0]);
                }
                break;
            }
        }

        // Prevent infinite loops
        if contour.len() > boundary_set.len() * 2 {
            break;
        }
    }

    if contour.len() < 3 {
        return None;
    }

    // Ensure path is closed properly
    if contour.len() < 3 {
        return None;
    }
    
    let first = contour[0];
    let last = *contour.last().unwrap();
    let dist = ((last.x - first.x).powi(2) + (last.y - first.y).powi(2)).sqrt();
    if dist > 0.5 {
        // Not closed, add first point
        contour.push(first);
    }

    // Don't filter out small paths - they might be valid small components
    // Only filter if it's truly invalid (less than 3 points)
    if contour.len() < 3 {
        return None;
    }

    Some(contour)
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

// Convert points to SVG path - simple and reliable
fn points_to_path(points: &[Point], options: &VectorizeOptions) -> String {
    if points.len() < 2 {
        return String::new();
    }

    let mut path = String::new();
    let smoothness = options.smoothness.clamp(0.0, 1.0);

    // Start path
    write!(path, "M {:.2} {:.2}", points[0].x, points[0].y).ok();
    
    // For logo mode with high smoothness, use curves; otherwise use lines
    if matches!(options.mode, VectorizeMode::Logo) && smoothness > 0.5 && points.len() > 4 {
        // Use smooth cubic Bézier curves for logos
        for i in 1..points.len() {
            let p0 = points[i - 1];
            let p1 = points[i];
            
            if i == points.len() - 1 {
                // Last point - line to close
                write!(path, " L {:.2} {:.2}", p1.x, p1.y).ok();
            } else {
                let p2 = points[i + 1];
                
                // Calculate control points for smooth curve
                let dx1 = p1.x - p0.x;
                let dy1 = p1.y - p0.y;
                let dx2 = p2.x - p1.x;
                let dy2 = p2.y - p1.y;
                
                // Control points extend from p1 towards p0 and p2
                let cp1x = p1.x - dx1 * smoothness * 0.3;
                let cp1y = p1.y - dy1 * smoothness * 0.3;
                let cp2x = p1.x + dx2 * smoothness * 0.3;
                let cp2y = p1.y + dy2 * smoothness * 0.3;
                
                write!(path, " C {:.2} {:.2} {:.2} {:.2} {:.2} {:.2}", 
                    cp1x, cp1y, cp2x, cp2y, p1.x, p1.y).ok();
            }
        }
    } else {
        // Simple polyline for accuracy
        for p in points.iter().skip(1) {
            write!(path, " L {:.2} {:.2}", p.x, p.y).ok();
        }
    }
    
    path.push_str(" Z");
    path
}

fn opacity_from_options(alpha: u8, _options: &VectorizeOptions) -> f32 {
    // For vectorization, we want full opacity based on the alpha channel
    // Don't use smoothness to affect opacity - that was causing paths to be invisible
    let base = alpha as f32 / 255.0;
    base.max(0.95) // Ensure paths are visible (at least 95% opacity for non-transparent pixels)
}

fn to_hex(color: [u8; 4]) -> String {
    let mut s = String::with_capacity(6);
    write!(&mut s, "{:02x}{:02x}{:02x}", color[0], color[1], color[2]).ok();
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{codecs::png::PngEncoder, ColorType, DynamicImage, ImageEncoder};
    use serde_json::json;

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
}
