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



Non-goals:

Photorealistic image vectorization. Photographs convert without falling over but
produce thousands of shapes, which is the wrong representation for them.

Full-blown vector editor.

A learned model. This was considered and is not needed: the failures that
motivated it were missing deterministic geometry, not a missing model. Sub-pixel
coverage reading, corner reconstruction and primitive fitting recover the
geometry directly, and are exactly measurable against known ground truth. Any
future model would have to beat those numbers on the same benchmark to justify
the dependency, the model weights and the loss of determinism.



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

Input: raster image (logo/icon/flat art)
Process (core engine):

1. Build a palette from interior pixels only.


2. Decompose into per-colour sub-pixel coverage fields.


3. Extract contours at the 0.5 coverage isoline.


4. Detect corners and straight runs; fit lines, curves and primitives.


5. Rasterize each candidate and score it against the source coverage.


6. Keep the simplest candidate that measures well enough; emit SVG.



Note that anti-aliasing is not a separate correction step. It is the input to
step 2, and the reason sub-pixel accuracy is available at all. The original plan
treated it as a post-hoc "edge adjustment" applied to integer-grid contours; that
ordering cannot recover the information, because it has already been discarded by
the time the contour exists.

Output: SVG string/file with:

Real <circle>, <ellipse> and <rect> elements where the geometry warrants them.

Clean <path> elements elsewhere, with holes cut by even-odd fill.

Minimal node count for a stated accuracy, chosen by measurement.

Grouping by colour, painted largest-area first.


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

4. Core Algorithm Pipeline (as implemented)

The engine treats an anti-aliased pixel as a measurement of coverage rather than
as a colour needing a palette slot. Every stage below lives in its own module
under png2svg/core/src/.

4.1 Palette (quantize.rs)

Classify pixels as interior (they and their four neighbours carry essentially the
same premultiplied colour) or not. Build the palette from interior pixels only:
anti-aliased edge pixels form a continuum between the real colours, and feeding
them to a quantizer produces phantom halo colours along every boundary. Fall back
to progressively looser samples when an image has too little flat area (thin
strokes, gradients).

Median cut on a bounded histogram, then agglomerative merging of near-identical
colours. Merging must be agglomerative rather than a single sequential pass: with
colours 197, 203, 199, 201 a sequential pass compares 203 against 197, finds them
too far apart, and strands them in separate groups even though the intermediate
values would have chained them together. A noisy source then fragments one flat
fill into hundreds of shapes.

4.2 Coverage decomposition (field.rs)

Compositing is linear in premultiplied-alpha space, so a pixel on a boundary
between palette colours A and B satisfies

    pixel = t * A + (1 - t) * B

and t is exactly the fraction of the pixel that A covers. Solve for the pair of
palette entries whose connecting segment passes closest to the pixel's colour,
and split the pixel's coverage accordingly. The result is one coverage field per
colour, summing to 1.0 at every pixel.

The pair search must try several starting points rather than committing to the
single nearest entry. A half-covered red pixel over transparency sits at the
midpoint of [transparent, red], and that midpoint can be numerically closer to a
third colour than to either end - which would attribute the whole anti-aliased rim
of a red mark to that third colour.

4.3 Contour extraction (trace.rs)

Marching squares over each coverage field at level 0.5, with vertices placed by
linear interpolation, giving sub-pixel boundary positions. Saddle cells are
disambiguated by the cell centre. The field is padded with zeros one sample
outside the image so a region running off the canvas closes exactly on the border.

Pixel art takes a different path: trace the exact edges between filled and empty
cells, with no interpolation and no chamfering.

Contours are then classified into outlines and holes by nesting depth, which
handles arbitrary nesting - a dot inside the hole of a ring is a filled outline
again.

4.4 Corners (corner.rs)

A single turn-angle threshold cannot tell a corner from a tight arc, because a
small circle turns as fast as a corner does. The discriminator is scale
behaviour: measure the turn over a short window and again over a window twice as
long. A corner concentrates all its turning at one point, so both report the same
angle; an arc spreads it, so doubling the window doubles the angle.

    corner:  angle(d) / angle(2d) ~ 1.0
    arc:     angle(d) / angle(2d) ~ 0.5

That ratio is scale invariant, so one threshold works for a 6px icon and a 2000px
mark alike.

4.5 Fitting (fit.rs)

Break the ring at corners and at the ends of long straight runs, then fit each
span. Straight runs are found by incremental second moments, tested against the
tracer's own noise floor rather than the caller's tolerance - whether a side is
straight is a property of the artwork, not of the error budget.

Curved spans use Schneider's least-squares cubic fit with Newton-Raphson
reparameterization. Tangents are taken one-sided at corners so corners stay sharp,
and from the neighbouring line at a smooth junction so nothing kinks.

The step that matters most: marching squares chamfers a sharp corner into two
45-degree steps straddling the true vertex, which no curve fitting will sharpen
back up. So straight runs are fitted from their interiors, with the chamfered ends
trimmed away, and the corner is recovered exactly by intersecting the two fitted
lines.

A final pass collapses cubics that never leave their own chord into lines, and
merges collinear neighbours.

4.6 Primitives (primitive.rs)

Circle by Kasa's algebraic fit polished with Landau's fixed-point iteration.
Ellipse by area-moment matching, which avoids the generalized eigenproblem a
direct conic fit would need. Axis-aligned rectangles from an already-fitted
four-line contour. Each fit reports its own worst-case error so the caller decides
whether to accept it.

4.7 Measure and choose (raster.rs, vectorize.rs)

For each region, generate candidates - primitives plus curve fits at a ladder of
tolerances - render each back to coverage with an anti-aliased scanline
rasterizer, and score it against the coverage measured from the image. Keep the
simplest candidate that measures well enough.

Three details make this work rather than merely sound good:

- The comparison is restricted to the pixels a shape is answerable for. A colour's
  coverage field describes every region of that colour, so an unrestricted
  comparison lets a neighbouring region, or a speck deliberately dropped as noise,
  pin every candidate's error at the maximum and destroy discrimination.
- The acceptance floor comes from the path candidates only. Primitives are
  hypotheses under test; if the floor were the best score overall, a shape where
  every candidate scores badly would let the simplest bad candidate set its own
  pass mark.
- The error budget scales with the shape's own size. Coverage error at a boundary
  is roughly the geometric displacement in pixels; half a pixel is invisible on a
  200px mark and is the entire shape on a 4px one.

4.8 Output (svg.rs)

Primitives are emitted as real <circle>, <ellipse> and <rect> elements rather than
flattened into path data, because that is what makes the output editable. Shapes
with holes become even-odd paths. Paths omit repeated command letters. Colours
covering more of the canvas paint first, so detail lands on top.

---

5. Roadmap

Milestones 1 through 4 (repo skeleton, palette and region map, region extraction,
simplification and curves) and milestone 5 (CLI) are complete, as is the quality
work originally listed under milestone 7. See TODO.md for the current checklist.

Remaining: milestone 6 (WASM build and web UI), plus gradient detection, stroke
recovery and a corpus of real logos with committed expected outputs.

Testing approach: the engine is validated against geometry whose exact values are
known. png2svg/core/tests/quality.rs renders known shapes with proper
anti-aliasing, vectorizes the pixels, and asserts the original geometry came back
- "did the circle return as a circle, with the right radius, to within a tenth of
a pixel". png2svg/core/examples/benchmark.rs reports the same measurements as a
table, and doubles as the harness for comparing against any other engine.

---

6. Stretch Features (Future)

Do not implement until everything above is stable.

Gradient detection, emitting <linearGradient> / <radialGradient>.

Stroke recovery: recognise a filled outline that was originally a stroked path.

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
