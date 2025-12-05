Project: Open-Source High-Quality PNG → SVG Converter

1. Overview

We’re building a high-quality, fully free and open-source PNG to SVG converter aimed at logos, icons, flat illustrations, and simple artwork.

Key goals:

Produce clean, smooth, edit-friendly SVGs (few nodes, good curves).

Handle anti-aliased raster images (logos, UI assets) gracefully.

Offer:

a CLI for devs / pipelines

a Web UI with real-time preview.


Use a single Rust core engine compiled to:

native binary (CLI / server)

WebAssembly (browser).



Non-goals (for now):

Photorealistic image vectorization.

Full-blown vector editor.

AI-heavy stack in v1 (optional later).



---

2. Tech Stack

Core engine

Language: Rust (edition 2021+)

Main crates (suggested):

image – for reading PNGs

clap – CLI argument parsing

thiserror / anyhow – error handling

serde / serde_json – config (if needed)

wasm-bindgen / wasm-pack – WebAssembly bindings



Web UI

Frontend: React + TypeScript

Build tool: Vite

Styling: Tailwind CSS 3.4.x (pin this, do NOT upgrade to 4.x)

Optionally:

react-dom, @types/react, etc.

Minimal state management via React hooks (no Redux etc. for now).



Repo structure (monorepo)

png2svg/
  core/           # Rust library crate - core engine
  cli/            # Rust binary crate - CLI wrapper around core
  web-ui/         # Vite + React + TS frontend, using wasm build of core
  examples/       # Sample PNGs + expected SVG results


---

3. High-Level Architecture

Flow

Input: PNG (logo/icon/flat art)
Process (core engine):

1. Pre-process image (normalize, optional denoise/downscale).


2. Color quantization & region labeling.


3. Contour extraction for each region.


4. Path simplification (RDP, Bézier curve fitting).


5. Anti-alias aware edge adjustment.


6. SVG generation (paths, groups, ordering).



Output: SVG string/file with:

Clean <path> elements.

Minimal but smooth node count.

Reasonable grouping by color / region.


Components

1. core/:

Image loading, quantization, segmentation, vectorization.

Exports a Rust API like:

pub struct VectorizeOptions {
    pub max_colors: u32,
    pub mode: VectorizeMode,  // Logo, Poster, PixelArt (for future)
    pub simplification_tolerance: f32,
    pub smoothness: f32,
}

pub fn png_to_svg(png_bytes: &[u8], options: &VectorizeOptions) -> Result<String, VectorizeError>;



2. cli/:

Wraps core and exposes options via CLI.

Reads PNG file → calls png_to_svg → writes SVG to file or stdout.



3. web-ui/:

React app:

PNG upload

Calls WASM version of png_to_svg

Shows input PNG + live SVG preview side by side

Sliders for colors, detail, smoothness

“Download SVG” button.






---

4. Core Algorithm Pipeline (Implementation Guidance)

Codex/Cursor: implement this pipeline step-by-step in core/.

4.1 Pre-processing

Load PNG into RGBA buffer using image crate.

Convert to a consistent color representation (e.g. linear RGB).

Optional:

Downscale if resolution is extremely large (configurable).

Apply mild denoising to reduce JPEG artifacts (if needed, later).



4.2 Color Quantization & Region Map

Use color quantization to reduce image to N colors.

Start with median cut or k-means clustering.

N is user-configurable (max_colors).


Assign each pixel to nearest palette color.

Build a label map for the image: each pixel belongs to a color index (0..N-1).


4.3 Region Extraction (Connected Components)

For each palette color, find connected components in the label map:

Use 4- or 8-connectivity (8 is preferable for smoother boundaries).

Each connected component = one region to vectorize.


Store each region as a binary mask or list of pixel coordinates.


4.4 Contour Tracing

For each region, find its outer contour(s) using something like:

Marching squares OR border-following (e.g. Moore-neighbor tracing).


Represent contour as an ordered list of 2D points (pixel coordinates).

Initially, this will be a high-resolution polygon approximating the pixel boundary.


4.5 Path Simplification

Use Ramer–Douglas–Peucker (RDP) or Visvalingam–Whyatt algorithm to reduce point count:

Input: list of points from contour.

Output: simplified list with fewer points while preserving shape.

Tolerance controlled by simplification_tolerance.


After simplification, fit cubic Bézier curves to sequences of points:

Use a simple least-squares Bézier curve fitting algorithm.

Keep corner points as actual corners (no over-smoothing).

Use smoothness to control curve vs straight segments.



4.6 Anti-Alias Aware Edge Adjustment (Basic v1)

In v1, implement a lightweight version:

For pixels near region boundaries:

Inspect alpha and neighboring colors to estimate where the “true” edge lies.


Adjust contour points slightly (sub-pixel shift) based on gradients, so edges look smoother and align with visual center of anti-aliasing.


This can be simple at first:

Compute a centroid/offset per boundary segment based on neighbor alpha values and apply a small correction.


4.7 SVG Generation

For each region, generate an SVG <path>:

Convert contour (with Bézier curves) into d="M ... C ... Z" commands.

Apply fill color from region palette.


Group paths logically:

By color: <g id="color-#rrggbb">...</g>

Possibly by region index for easier debugging.


Export final SVG string with:

viewBox based on original image dimensions.

Optionally, width/height attributes.




---

5. Roadmap & Milestones

Codex/Cursor: follow this order when implementing.

Milestone 1 – Repo & Skeleton

[ ] Create monorepo structure:

png2svg/
  core/
  cli/
  web-ui/
  examples/

[ ] Initialize Rust library crate in core/.

[ ] Initialize Rust binary crate in cli/ depending on core/.

[ ] Initialize Vite + React + TS app in web-ui/.

[ ] Add basic README.md describing project & goals.



---

Milestone 2 – Basic Core: PNG → Region Map

In core/:

[ ] Add image crate & implement load_png_from_bytes helper.

[ ] Implement VectorizeOptions and VectorizeMode enums.

[ ] Implement basic color quantization:

[ ] Collect pixels.

[ ] Apply median cut or k-means to get palette.

[ ] Map pixels to nearest palette color index.


[ ] Implement a debug function:

[ ] Export quantized image as PNG (for testing).


[ ] Expose a function:

pub fn quantize_image(png_bytes: &[u8], options: &VectorizeOptions) -> Result<QuantizedImage, VectorizeError>;



---

Milestone 3 – Region Extraction & Simple SVG

In core/:

[ ] Implement connected-component labeling on the label map.

[ ] For each component, find boundary/contour (polygonal).

[ ] Implement polygonal SVG export (no simplification yet):

[ ] Convert raw contours directly to L commands.

[ ] Basic SVG generator producing one <path> per region.


[ ] Wire it together in png_to_svg (no simplification, no curves, just polygons).


This will produce very detailed SVGs but is a good correctness check.


---

Milestone 4 – Path Simplification & Curves

In core/:

[ ] Implement RDP simplification on the contour polygons.

[ ] Implement basic cubic Bézier fitting on simplified segments.

[ ] Add simplification_tolerance and smoothness parameters to VectorizeOptions.

[ ] Use Bézier curves (C/Q) in SVG path output.

[ ] Ensure no extreme oversmoothing around sharp corners.


This milestone should already outperform many low-quality converters.


---

Milestone 5 – CLI Tool

In cli/:

[ ] Add clap for argument parsing.

[ ] Implement:

png2svg input.png -o output.svg \
  --colors 8 \
  --mode logo \
  --tolerance 2.0 \
  --smoothness 0.8

[ ] Options:

--colors / -c: max colors

--mode / -m: logo, poster, pixel-art (pixel-art can behave like logo but without smoothing initially)

--tolerance / -t: simplification tolerance

--smoothness / -s: curve smoothness


[ ] Proper error messages & exit codes.



---

Milestone 6 – WebAssembly & Web UI

6.1 WASM Build

In core/:

[ ] Add wasm-bindgen and create wasm feature flag.

[ ] Create a WASM entry function:

#[wasm_bindgen]
pub fn png_to_svg_wasm(png_bytes: &[u8], options_json: &str) -> String;

options_json will contain the serialized VectorizeOptions.


[ ] Create a build process using wasm-pack or Vite plugin to produce a .wasm + JS glue module for web-ui.


6.2 Web UI

In web-ui/:

[ ] Setup Vite + React + TS + Tailwind 3.4.x.

[ ] Add UI components:

[ ] File upload (PNG).

[ ] Sliders:

colors (e.g., 2–32)

detail level / tolerance

smoothness


[ ] Mode selector: Logo, Poster, Pixel Art (even if they internally map to same options initially).

[ ] Preview area:

left: original PNG

right: rendered SVG (use <svg> rendered via dangerouslySetInnerHTML or react-svg).


[ ] Download SVG button.


[ ] Wire up WASM:

[ ] On file load + parameter change, call png_to_svg_wasm.

[ ] Show loading state while vectorization runs.




---

Milestone 7 – Quality Improvements & Polish

[ ] Improve anti-alias handling:

[ ] Use alpha + neighboring colors to adjust boundary locations.


[ ] Add grouping in SVG:

[ ] Group paths by color.

[ ] Add IDs to groups for debugging.


[ ] Add preset buttons:

[ ] “Logo (Clean)”

[ ] “Poster (More Detail)”

[ ] “Pixel Art (Crisp)”


[ ] Add examples/ with sample PNGs + generated SVGs.

[ ] Document trade-offs and recommended settings in README.md.



---

6. Stretch Features (Future)

Do not implement in v1 unless everything above is stable.

AI/ML edge refinement (small model to detect important edges).

Interactive editing in the web UI:

Click shape → highlight path.

Toggle visibility or remove tiny specks.

Merge regions via UI.


Server mode:

HTTP API for batch conversion or pipelines.


SVG optimization integration (e.g. SVGO-like passes).



---

7. Summary for Codex/Cursor

Focus order:

1. Set up Rust core + CLI with a simple but correct PNG → basic SVG pipeline.


2. Add quantization → regions → contours → simplification → curves.


3. Wrap core in WASM and build React + Tailwind 3.4.x UI that calls it.


4. Iterate on quality (simplification, grouping, anti-alias awareness).
