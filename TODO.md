# Vectorizer TODO

A living checklist to track the next steps toward the full curve-based pipeline and web experience.

## Core engine (Rust)
- [ ] Swap histogram palette for a smarter quantizer (median cut/k-means) with an options surface.
- [ ] Connected-component labeling to identify regions for contour tracing.
- [ ] Contour tracing per region with winding info and hole detection.
- [ ] Path simplification using tolerance-driven RDP and Bézier fitting.
- [ ] Anti-alias aware boundary adjustment that uses alpha/neighbor colors.
- [ ] SVG output that groups paths by color, with stable IDs for debugging.

## CLI
- [ ] Expose all tunable options (tolerance, min-region area, presets) with clear help text.
- [x] Polished exit codes and error messaging for bad inputs.

## WASM + web UI
- [ ] Reusable wasm-pack/Vite build that emits the WASM bundle consumed by the app.
- [ ] Wire the “Download SVG” button to wasm output and loading states.
- [ ] Preset buttons for Logo/Poster/Pixel Art and documented parameter ranges.
- [ ] Example gallery (PNG input + expected SVG) for quick validation.

## Documentation
- [x] README updates with option descriptions, presets, and quality trade-offs.
- [ ] Developer notes for running the WASM build and publishing the package.
