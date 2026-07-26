# Open Vectorizer

A fully open-source raster → SVG converter for logos, icons and flat artwork. Rust core, CLI, and a WebAssembly build for the browser. Runs entirely locally: no model, no inference, no network.

## Status

The vectorization engine works. It recovers circles, ellipses, rectangles, sharp corners and smooth curves from anti-aliased raster input, and emits minimal, editable SVG. The three cases that defeated the previous implementation — smooth rings, sharp logo marks, and noisy transparent edges — are now covered by ground-truth tests.

Measured against shapes whose exact geometry is known (`cargo run --release -p png2svg-core --example benchmark`):

| case | nodes | primitives | accuracy | geometry error |
| --- | --- | --- | --- | --- |
| circle | 1 | `<circle>` | 0.99991 | 0.0026px |
| circle, off-grid centre and radius | 1 | `<circle>` | 0.99991 | 0.0047px |
| ring | 2 | 2 × `<circle>` | 0.99986 | 0.0014px |
| ellipse | 1 | `<ellipse>` | 0.99991 | – |
| axis-aligned square | 1 | `<rect>` | 0.99946 | – |
| rotated square | 4 | 4 lines | 0.99994 | – |
| triangle | 3 | 3 lines | 0.99998 | – |
| 5-point star | 10 | 10 lines | 0.99979 | – |
| rounded rectangle | 8 | 4 lines + 4 curves | 0.99954 | – |
| 24px icon | 1 | `<circle>` | 0.99876 | 0.0014px |

`accuracy` is 1 − mean absolute coverage error after re-rasterizing the SVG and comparing it to the source. Node counts and accuracy belong together: any vectorizer can buy accuracy with more geometry.

Still to do: the WebAssembly build wiring and the web UI. See `TODO.md`.

## Approach

The engine treats an anti-aliased pixel as a **measurement of coverage**, not as a colour that needs a palette slot.

```
image
  → palette built from interior pixels only
  → per-colour sub-pixel coverage fields
  → contours at the 0.5 coverage isoline
  → corners, straight runs, primitive hypotheses
  → candidate outlines, simple to complex
  → rasterize each candidate, compare against the source coverage
  → keep the simplest candidate that measures well enough
  → SVG
```

Four things follow from that, and together they are what make the output clean:

**Anti-aliasing is information, not noise.** Compositing is linear in premultiplied-alpha space, so a pixel on a boundary between colours `A` and `B` satisfies `pixel = t·A + (1−t)·B`, and `t` *is* the fraction of the pixel that `A` covers. Recovering `t` places a boundary to a hundredth of a pixel. An integer-grid tracer cannot do better than half a pixel — which is 4% of a 24px icon.

**The palette is built from interior pixels only.** Anti-aliased edge pixels form a continuum between the real colours of an image. Feed them to a quantizer and they steal palette slots, producing phantom halo colours along every boundary. Excluding them first means the palette describes the colours a designer actually used.

**Corners are reconstructed, not traced.** Marching squares chamfers a sharp corner: a 90° turn comes back as two 45° steps straddling the true vertex, and no amount of curve fitting sharpens that back up. So straight runs are fitted from their *interiors*, with the chamfered ends trimmed away, and the corner is recovered exactly by intersecting the two fitted lines. That is why a rotated square comes back as four lines meeting within 0.2px of the true corners.

**Nothing is decided by a guessed threshold.** For each region the engine generates candidates — circle, ellipse, rectangle, and curve fits at a ladder of tolerances — then renders each one back to coverage and scores it against what was measured from the image. The simplest candidate that measures well enough wins. Whether a region "is" a circle is settled by measurement, so a genuine circle becomes `<circle cx cy r>` while a 4px hard-edged square, whose best-fit circle is only slightly wrong in absolute terms, does not.

## Repository layout

- `png2svg/core/` – the engine (`png2svg-core`)
- `png2svg/cli/` – command-line wrapper (`png2svg-cli`)
- `web-ui/` – React + TypeScript + Tailwind front-end (placeholder)
- `examples/` – sample inputs and outputs

Inside the core, one module per stage: `quantize` (palette), `field` (coverage), `trace` (contours), `corner`, `fit` (curves), `primitive`, `raster` (the compare loop), `svg` (output), `vectorize` (orchestration).

## Getting started

Requires a Rust toolchain (edition 2021+).

```bash
cargo test --workspace
```

### CLI

```bash
cargo run --release -p png2svg-cli -- logo.png -o logo.svg
```

Add `--stats` to see what it produced and how well it matches:

```
[open-vectorizer] 1 shapes, 2 nodes (2 circles, 0 ellipses, 0 rects), accuracy 0.99913
```

#### Options

- `--mode` (`auto` | `logo` | `poster` | `pixel`, default `auto`) — `auto` inspects the image and picks. `pixel` traces exact cell edges and never smooths, so pixel art stays on the grid.
- `--colors` (`2`–`64`, default `8`) — palette size ceiling. Near-identical colours are merged, so asking for more than the artwork uses is harmless.
- `--detail` (`0.0`–`1.0`, default `0.5`) — how much small structure to keep. Drives speck removal.
- `--smoothness` (`0.0`–`1.0`, default `0.5`) — how much evidence is needed before a curve is broken by a corner.
- `--tolerance` (`0.1`–`10.0`, default `1.5`) — geometric error **ceiling**. A quarter of this is the budget in pixels, so the default allows about 0.38px. The engine may fit more tightly than asked when the measurement demands it, but never looser.

Out-of-range values are rejected with a clear message.

### Benchmark

```bash
cargo run --release -p png2svg-core --example benchmark
```

Renders known geometry, vectorizes it, and reports recovered geometry, node counts, accuracy and timing. This is also the harness for comparing against another engine: run the same inputs through it and compare the `nodes` and `accuracy` columns.

### Samples

```bash
cargo run --release -p png2svg-core --example generate_samples
```

Writes PNG inputs and SVG outputs to `target/vectorizer-samples/`.

## Performance

Single-threaded, on one core. Logo-shaped input:

| size | time |
| --- | --- |
| 128px | ~10ms |
| 512px | ~100ms |
| 1024px | ~400ms |
| 2048px | ~1.5s |

Roughly linear in pixel count. Output is deterministic: the same input and options always produce byte-identical SVG.

## Scope

Built for logos, icons, stickers, flat illustrations, line art and pixel art.

Photographs are a non-goal. They will convert without falling over — a 1024px photographic input takes about 2s — but the result is thousands of shapes, which is the wrong representation for that kind of image.

Other current limits, stated plainly:

- **No gradient detection.** A gradient becomes a set of quantized bands rather than a `<linearGradient>`.
- **No stroke recovery.** A stroked outline comes back as a filled shape following both sides of the stroke, not as a `stroke-width`.
- **No text recognition.** Letterforms are vectorized as shapes, which is usually what you want from a logo, but they are not fonts.
- **Rotated rectangles stay paths.** They are recovered as four exact lines; only axis-aligned ones become `<rect>`.
- **Pixel-art detection is conservative.** `auto` only chooses `pixel` for images up to 16px with no partial alpha. Pass `--mode pixel` for larger pixel art.

## Licence

MIT. See `LICENSE`.
