# Open Vectorizer

A fully open-source raster → SVG converter for logos, icons and flat artwork. Rust core, CLI, and a WebAssembly build for the browser. Runs entirely locally: no model, no inference, no network.

**Try it in the browser: [vector.vardir.no](https://vector.vardir.no)** — the engine is compiled to WebAssembly and runs on the page, so the image is never uploaded anywhere. Also served at [vardirhq.github.io/open-vectorizer](https://vardirhq.github.io/open-vectorizer/).

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

Still to do: gradient detection, stroke recovery, and a corpus of real logos. See `TODO.md`.

## Compared with other engines

Against the two leading open-source non-AI vectorizers, on the same inputs, scored the same way. Every result is rendered back with `rsvg-convert` and compared to the source raster, so nothing in the scoring uses our own internals. Reproduce with `benchmarks/shootout/` — see `benchmarks/README.md`, which also lists the caveats in full.

**Shape geometry**, single-colour cases, coverage compared. We read the anti-aliased image; potrace 1.16 gets the ideal thresholded bilevel mask, the best input a 1-bit tracer can take.

| case | ours: accuracy / nodes / area | potrace: accuracy / nodes / area |
| --- | --- | --- |
| circle | 0.99986 / 1 / −3 | 0.99801 / 5 / −80 |
| circle off-grid | 0.99956 / 1 / −11 | 0.99718 / 5 / −72 |
| ring | 0.99966 / 8 / +6 | 0.99591 / 18 / −60 |
| ellipse | 0.99964 / 1 / −11 | 0.99761 / 8 / −84 |
| square | 0.99927 / 1 / +1 | 0.99633 / 8 / +13 |
| rotated square | 0.99980 / 3 / +2 | 0.99758 / 5 / −65 |
| triangle | 0.99995 / 2 / −0 | 0.99797 / 7 / −56 |
| 5-point star | 0.99972 / 9 / +7 | 0.99851 / 20 / +17 |
| rounded rect | 0.99957 / 8 / +12 | 0.99584 / 8 / −110 |
| capsule | 0.99945 / 8 / −7 | 0.99448 / 11 / −113 |
| 24px icon | 0.99705 / 1 / −2 | 0.97926 / 5 / −12 |
| organic blob | 0.99877 / 18 / −35 | 0.99760 / 17 / −84 |
| thick L | 1.00000 / 5 / +0 | 1.00000 / 12 / +0 |
| circle @ 1024px | 0.99989 / 1 / +90 | 0.99958 / 20 / −424 |

`area` is rendered minus source coverage, in pixels. potrace's consistently negative figures are the thresholding: with the anti-aliasing discarded, every boundary lands up to half a pixel inside where it belongs. That is the structural difference, not a tuning gap.

**Colour reproduction**, every case on an opaque white background — the fair common ground, since VTracer 0.6.5 is built for opaque input and "logo on white" is the commonest real input. Full RGBA compared. potrace is excluded; it cannot represent colour.

| case | ours: accuracy / nodes / ms | vtracer: accuracy / nodes / ms |
| --- | --- | --- |
| circle | 0.99991 / 8 / 27 | 0.99592 / 14 / 8 |
| circle off-grid | 0.99970 / 8 / 19 | 0.99483 / 13 / 6 |
| ring | 0.99977 / 16 / 34 | 0.99150 / 24 / 9 |
| ellipse | 0.99976 / 8 / 24 | 0.99382 / 19 / 8 |
| square | 0.99951 / 7 / 10 | 0.99195 / 8 / 6 |
| rotated square | 0.99987 / 9 / 12 | 0.99197 / 49 / 7 |
| triangle | 0.99997 / 7 / 13 | 0.99536 / 40 / 8 |
| 5-point star | 0.99981 / 21 / 28 | 0.99292 / 76 / 10 |
| rounded rect | 0.99962 / 19 / 17 | 0.99468 / 50 / 7 |
| capsule | 0.99963 / 19 / 15 | 0.99334 / 32 / 6 |
| 24px icon | 0.99804 / 8 / 4 | 0.97522 / 10 / 4 |
| organic blob | 0.99918 / 41 / 36 | 0.99535 / 34 / 11 |
| thick L | 1.00000 / 13 / 13 | 1.00000 / 10 / 8 |
| circle @ 1024px | 0.99993 / 8 / 540 | 0.99782 / 56 / 156 |
| badge, 3 colours | 0.99954 / 33 / 52 | 0.98939 / 32 / 10 |
| wordmark | 0.99995 / 20 / 28 | 0.99668 / 23 / 7 |
| three-colour mark | 0.99925 / 70 / 30 | 0.99364 / 183 / 11 |

We lead on accuracy in every case in both tables, and on node count in most. Where we do not: potrace matches us on the organic blob (17 nodes vs 18) — the one case with no primitive to exploit, and a fair reading of its curve fitting, which is genuinely excellent. VTracer beats us on the blob and the thick L, and is consistently **2–4× faster**.

### What this does and does not show

The cases are our own synthetic geometry, and they favour what this engine was built for: primitives, polygons, rounded rectangles. Real logos also contain glyphs, thin strokes, gradients, drop shadows and recompressed screenshots — none of which are in here, and several of which we do not handle at all (see Scope). An engine better at those would score worse on this benchmark.

So the honest claim is narrow: **for flat vector artwork with geometric structure, on inputs of this kind, this engine is ahead of both.** A corpus of real logos is the next thing needed to say anything broader, and it is tracked in `TODO.md`.

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

### Web UI

```bash
cd web-ui
npm install
npm run dev
```

Builds the WebAssembly bundle and starts Vite. Conversion runs in a Web Worker, so
the tab stays responsive; the browser decodes the file itself, so every format it
reads is supported and no image codec is compiled into the wasm — which keeps the
payload at 150KB gzipped. See `web-ui/README.md`.

## Performance

Single-threaded, on one core. Logo-shaped input:

| size | time |
| --- | --- |
| 128px | ~10ms |
| 512px | ~100ms |
| 1024px | ~400ms |
| 2048px | ~1.5s |

Roughly linear in pixel count. In the browser it is about 2× slower again — a 1024px logo takes roughly 550ms — which is why the web UI runs it in a worker. Output is deterministic: the same input and options always produce byte-identical SVG.

## Scope

Built for logos, icons, stickers, flat illustrations, line art and pixel art.

Photographs are a non-goal. They will convert without falling over — a 1024px photographic input takes about 2s — but the result is thousands of shapes, which is the wrong representation for that kind of image.

Other current limits, stated plainly:

- **No gradient detection.** A gradient becomes a set of quantized bands rather than a `<linearGradient>`.
- **No stroke recovery.** A stroked outline comes back as a filled shape following both sides of the stroke, not as a `stroke-width`.
- **No text recognition.** Letterforms are vectorized as shapes, which is usually what you want from a logo, but they are not fonts.
- **Rotated rectangles stay paths.** They are recovered as four exact lines; only axis-aligned ones become `<rect>`.
- **Pixel-art detection is conservative.** `auto` only chooses `pixel` for images up to 16px with no partial alpha. Pass `--mode pixel` for larger pixel art.
- **The web UI caps input at 2048px** on the long edge and downscales above that. Coverage is one full-canvas field per palette colour, so memory grows as `width × height × colours`, and 4096px would exceed what WebAssembly can hold on mobile. The CLI has more headroom but the same underlying cost.

## Licence

MIT. See `LICENSE`.
