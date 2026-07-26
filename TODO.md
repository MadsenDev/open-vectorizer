# Vectorizer TODO

Tracking what is done and what is left.

## Core engine (Rust)

- [x] Palette built from interior pixels, so anti-aliased edges never occupy palette slots.
- [x] Median-cut quantizer with agglomerative merging of near-identical colours.
- [x] Sub-pixel coverage decomposition: every pixel explained as a two-colour blend.
- [x] Contour extraction at the 0.5 coverage isoline, with saddle disambiguation.
- [x] Crisp cell-edge tracer for pixel art.
- [x] Hole and topology handling by nesting depth, so arbitrary nesting works.
- [x] Multi-scale corner detection that separates corners from tight arcs.
- [x] Least-squares cubic fitting (Schneider) with Newton-Raphson reparameterization.
- [x] Straight-run detection, with exact corners recovered by line intersection.
- [x] Primitive recovery: circle, ellipse, axis-aligned rectangle.
- [x] Anti-aliased rasterizer, used to score candidate geometry against the source.
- [x] Candidate selection by measurement rather than by threshold.
- [x] SVG output with real `<circle>` / `<ellipse>` / `<rect>` elements, grouped by colour.
- [x] Deterministic output.
- [ ] Gradient detection, emitting `<linearGradient>` / `<radialGradient>`.
- [ ] Stroke recovery: detect a filled outline that was originally a stroked path.
- [ ] Rounded-rectangle primitive (`<rect rx>`), currently fitted as lines plus curves.
- [ ] Better pixel-art detection than the current size-and-alpha heuristic (look for an integer upscale grid).
- [ ] Faster: we are 2-4x slower than VTracer. The pipeline is embarrassingly
      parallel per palette entry, and the coverage decomposition is the hot loop.
- [ ] Relative rather than absolute flatness in `interior_mask`. A low-contrast
      boundary steps by less than the fixed threshold, so its blend colours reach
      the palette; population-weighted merging now contains the damage, but the
      classifier itself is still the weak link.

## CLI

- [x] All tunable options exposed with accurate help text.
- [x] Polished exit codes and error messaging for bad inputs.
- [x] `--stats` reporting shape count, node count, primitives and accuracy.
- [ ] Batch mode for converting a directory.

## WASM + web UI

- [ ] Reusable wasm-pack/Vite build that emits the WASM bundle consumed by the app.
- [ ] Wire the "Download SVG" button to wasm output and loading states.
- [ ] Preset buttons for Logo/Poster/Pixel Art and documented parameter ranges.
- [ ] Show node count and accuracy in the UI, so the quality/complexity trade-off is visible.
- [ ] Example gallery (PNG input + expected SVG) for quick validation.

## Testing

- [x] Ground-truth quality suite: render known geometry, vectorize, check recovery.
- [x] Benchmark example reporting nodes, accuracy, primitives and timing.
- [x] Behavioural fixtures asserted through the `Document` API rather than SVG text.
- [x] Determinism tests.
- [x] Shootout harness comparing against potrace and VTracer on identical inputs,
      scored independently of our own internals (`benchmarks/shootout/`).
- [ ] A corpus of real logos with committed expected outputs, to catch regressions on
      the messy cases synthetic shapes do not cover (glyphs, thin strokes, drop shadows,
      recompressed screenshots).

## Documentation

- [x] README describing the approach, options, measured results and known limits.
- [x] Per-module documentation explaining why each stage works the way it does.
- [ ] Developer notes for running the WASM build and publishing the package.
