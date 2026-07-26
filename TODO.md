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

- [x] Reproducible wasm build (`web-ui/scripts/build-wasm.sh`) emitting the bundle the app consumes.
- [x] Browser build without image decoders: the page decodes natively, so every
      format it reads works and the payload is 150KB gzipped rather than 393KB.
- [x] Vectorization in a Web Worker, so the tab stays responsive.
- [x] Download button wired to real output, with loading and error states.
- [x] Preset buttons for Logo/Poster/Pixel Art, and every option documented in the UI.
- [x] Node count and accuracy shown, so the quality/complexity trade-off is visible.
- [x] Input above 2048px downscaled, with a notice, to stay inside wasm memory.
- [x] GitHub Pages deployment on push to `main`, at vector.vardir.no and the
      project-site URL. Relative asset paths, so one artifact serves from both.
- [x] Browser smoke test (`web-ui/scripts/smoke.mjs`).
- [x] Options as a bottom sheet below `lg`, so settings are reachable on a phone
      without scrolling, with the result kept visible while a slider moves.
- [x] Mobile smoke test covering the sheet (`web-ui/scripts/smoke-mobile.mjs`).
- [ ] Example gallery (PNG input + expected SVG) for quick validation.
- [ ] Side-by-side zoom and pan, so sub-pixel differences are actually visible.
- [ ] Raise the input ceiling: stream coverage per colour instead of holding every
      field at once, which is what forces the 2048px cap.

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
- [x] Developer notes for the WASM build and the Pages deployment (`web-ui/README.md`).
- [x] Contributor documentation: `CONTRIBUTING.md`, `docs/ARCHITECTURE.md`,
      `CODE_OF_CONDUCT.md`, and issue/PR templates.
- [ ] Publish `png2svg-core` to crates.io and the wasm package to npm.

## Community

- [x] Open problems written up as issues with enough context to start on, labelled
      `good first issue` / `help wanted`.
- [ ] Repository description and topics on GitHub (settings, not in the repo).
- [ ] Decide whether to enable Discussions for design conversations that are not
      issues.
