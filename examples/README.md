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

The Rust test suite generates small PNG fixtures in memory instead of committing binary images.
Those fixtures currently cover:

- transparent logo cutouts
- two-color logo regions
- crisp pixel-mode edges
- smoothed logo-mode paths

Run them with:

```bash
cargo test --workspace
```

## Next Example Step

Once the engine is stable enough, add real sample files here:

- `input/` for source PNG, JPG, and WebP files
- `output/` for generated SVG snapshots
- `preview/` for side-by-side screenshots used in the README
