# Open Vectorizer Examples

This directory is for logo-like raster fixtures and before/after conversion outputs.

The project is intentionally scoped toward vector-friendly assets:

- logos and marks
- icons
- stickers
- flat illustrations
- line art
- pixel art

It is not trying to vectorize photographs.

## Current Fixture Coverage

Fixtures are generated in memory rather than committed as binaries.

`png2svg/core/tests/quality.rs` renders shapes whose exact geometry is known, with
proper anti-aliasing, then checks that vectorizing the pixels recovers the
original: circles, rings, ellipses, squares, rotated squares, triangles, stars,
rounded rectangles, small icons, multi-colour marks, noisy edges, semi-transparent
fills, and pixel art.

`png2svg/core/tests/logo_fixtures.rs` covers behavioural cases: transparent
cutouts, multi-colour regions, crisp pixel-mode edges, speck removal, brand-colour
merging, and sub-pixel edge placement.

```bash
cargo test --workspace
```

## Generated Samples

```bash
cargo run --release -p png2svg-core --example generate_samples
```

Writes PNG inputs to `target/vectorizer-samples/inputs/` and SVG outputs to
`target/vectorizer-samples/svg/`. CI uploads these as artifacts.

## Benchmark

```bash
cargo run --release -p png2svg-core --example benchmark
```

Reports recovered geometry, node counts, accuracy and timing per case.

## Next Example Step

Add a corpus of real logos here, with committed expected outputs, to catch
regressions on the messy cases synthetic shapes do not cover: glyphs, thin
strokes, drop shadows, recompressed screenshots.

- `input/` for source PNG, JPG, and WebP files
- `output/` for generated SVG snapshots
- `preview/` for side-by-side screenshots used in the README
