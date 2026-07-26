# Architecture

A map of the engine for people who want to change it. The README explains what
the pipeline does and why; this explains where each part lives, what it hands to
the next stage, and where you would intervene to add something.

## The shape of it

```
RgbaImage
  │
  ├─ quantize.rs   build_palette      → Palette          (interior pixels only)
  ├─ field.rs      decompose          → Decomposition    (one coverage Field per colour)
  │
  └─ for each opaque palette entry:
       ├─ trace.rs   trace_level(0.5) → Vec<Vec<Point>>  (sub-pixel rings)
       │             trace_cells      → Vec<Vec<Point>>  (pixel-art path, exact cell edges)
       ├─ trace.rs   nest             → outlines + holes by nesting depth
       │
       └─ for each outline:
            ├─ corner.rs     detect_corners  → Vec<usize>   (indices into the ring)
            ├─ fit.rs        fit_contour     → Contour      (lines + cubics)
            ├─ primitive.rs  fit_circle / fit_ellipse / detect_rect → Outline
            │
            ├─ vectorize.rs  candidates_for  → Candidates   (simplest first)
            ├─ raster.rs     rasterize + compare_within     (score each candidate)
            └─ vectorize.rs  pick_candidate  → the simplest one that measures well enough
  │
  └─ svg.rs        Document::to_svg   → String
```

`vectorize.rs` is the orchestrator and the only place that knows the whole
sequence. Every other module is independently testable and, in principle,
independently replaceable.

## Core types

Worth knowing before reading any single module.

| Type | Where | What it is |
| --- | --- | --- |
| `Premul` | `quantize.rs` | A colour in premultiplied alpha. Compositing is linear in this space, which is the entire basis of the coverage decomposition. |
| `Palette` | `quantize.rs` | The colours a designer actually used, built from interior pixels only. |
| `Field` | `field.rs` | One `f32` per pixel: how much of that pixel a given palette colour covers. Fields sum to 1.0 at every pixel. |
| `Decomposition` | `field.rs` | One `Field` per palette entry, plus the labelling used by the pixel-art path. |
| `Point` | `geom.rs` | `f64` pair. Positions are sub-pixel throughout; nothing is snapped to the integer grid except in crisp mode. |
| `Segment` | `path.rs` | `Line` or `Cubic`. The only two things a fitted contour is made of. |
| `Contour` | `path.rs` | A closed ring of segments, with a start point. |
| `Outline` | `path.rs` | A `Contour` **or** a primitive (`Circle`, `Ellipse`, `Rect`). This is the unit candidates are chosen between. |
| `Shape` | `path.rs` | One outline plus its holes, in one colour. |
| `Document` | `svg.rs` | Shapes in paint order, plus canvas size. The behavioural test surface — fixtures assert against this, not against SVG text. |
| `Mask` | `raster.rs` | Rendered coverage for a candidate, over a window rather than the whole canvas. |

## Stage by stage

### 1. Palette — `quantize.rs`

`interior_mask` classifies a pixel as interior when it and its four neighbours
carry essentially the same premultiplied colour. `build_palette` then runs median
cut over interior pixels only, followed by **agglomerative** merging of
near-identical colours.

Two things here are load-bearing:

- Edge pixels must be excluded before quantizing. Anti-aliased edges form a
  continuum between real colours; fed to a quantizer they take palette slots and
  produce phantom halo colours along every boundary.
- Merging must be agglomerative, not a single sequential pass. With colours 197,
  203, 199, 201 a sequential pass compares 203 against 197, finds them too far
  apart, and strands them apart even though the intermediates would have chained
  them. A noisy source then fragments one flat fill into hundreds of shapes.

**Known weak point:** the flatness test in `interior_mask` is an absolute
threshold. A low-contrast boundary steps by less than it, so the blend colours
reach the palette. Population-weighted merging contains the damage but the
classifier is still the weak link. A relative test is an open problem.

### 2. Coverage — `field.rs`

For each pixel, find the pair of palette entries whose connecting segment passes
closest to the pixel's colour, and split the pixel's coverage along it:
`pixel = t·A + (1−t)·B`, where `t` is exactly the fraction of the pixel that `A`
covers.

The pair search tries several starting points rather than committing to the
nearest entry. A half-covered red pixel over transparency sits at the midpoint of
`[transparent, red]`, and that midpoint can be numerically closer to some third
colour than to either end — which would attribute the entire anti-aliased rim of
a red mark to that third colour.

This is the hot loop. It is also the memory ceiling: coverage is one full-canvas
field per colour, so cost grows as `width × height × colours`. That is what
forces the web UI's 2048px cap.

### 3. Contours — `trace.rs`

Marching squares over each field at level 0.5, with vertices placed by linear
interpolation, so boundaries land between pixel centres. Saddle cells are
disambiguated by the cell centre. The field is padded with zeros one sample
outside the image, so a region running off the canvas closes exactly on the
border.

Pixel art takes `trace_cells` instead: exact edges between filled and empty
cells, no interpolation, no chamfering.

`nest` then classifies rings into outlines and holes by nesting depth, so
arbitrary nesting works — a dot inside the hole of a ring is a filled outline
again.

### 4. Corners — `corner.rs`

A single turn-angle threshold cannot separate a corner from a tight arc, because
a small circle turns as fast as a corner does. The discriminator is scale
behaviour: measure the turn over a window of length `d` and again over `2d`.

```
corner:  angle(d) / angle(2d) ≈ 1.0    (all the turning is at one point)
arc:     angle(d) / angle(2d) ≈ 0.5    (the turning is spread out)
```

That ratio is scale invariant, so one threshold serves a 6px icon and a 2000px
mark alike.

### 5. Fitting — `fit.rs`

The largest module, and the one with the most room in it. The ring is broken at
corners and at the ends of long straight runs, and each span is fitted.

Straight runs are found by incremental second moments, tested against the
*tracer's* noise floor rather than the caller's tolerance — whether a side is
straight is a property of the artwork, not of the error budget. Curved spans use
Schneider's least-squares cubic fit with Newton-Raphson reparameterization.
Tangents are one-sided at corners so corners stay sharp, and taken from the
neighbouring line at a smooth junction so nothing kinks.

**The step that matters most:** marching squares chamfers a sharp corner into two
45° steps straddling the true vertex, and no amount of curve fitting sharpens
that back up. So straight runs are fitted from their *interiors*, with the
chamfered ends trimmed away, and the corner is recovered exactly by intersecting
the two fitted lines. That is why a rotated square returns as four lines meeting
within 0.2px of the true corners.

`straighten` finally collapses cubics that never leave their own chord into
lines, and merges collinear neighbours.

### 6. Primitives — `primitive.rs`

- **Circle:** Kasa's algebraic fit, polished with Landau's fixed-point iteration.
- **Ellipse:** area-moment matching, which avoids the generalized eigenproblem a
  direct conic fit would need.
- **Rectangle:** recognised from an already-fitted four-line contour, axis-aligned
  only.

Each fit reports its own worst-case error. It does not decide whether it is good
enough — the caller does, by measurement.

### 7. Candidates and selection — `vectorize.rs`, `raster.rs`

This is the part that makes the output clean, and the part to understand before
changing anything upstream of it.

`candidates_for` produces a list of `Outline`s for one ring: primitives that pass
a cheap algebraic gate, plus curve fits at every step of a tolerance ladder. The
list is sorted simplest-first, stably, so primitives keep priority over equally
cheap paths and the whole selection stays deterministic.

`build_shape` then starts from the most faithful candidate for every ring and
simplifies each in turn while the whole shape still measures well enough. Each
trial renders the candidate with `raster::rasterize` and scores it against the
coverage measured from the image.

Three details make this work rather than merely sound good:

- **The comparison is restricted to the pixels a shape is answerable for**
  (`scope_of`). A colour's field describes every region of that colour, so an
  unrestricted comparison lets a neighbouring region — or a speck deliberately
  dropped as noise — pin every candidate's error at the maximum and destroy
  discrimination.
- **The acceptance floor comes from the path candidates only.** Primitives are
  hypotheses under test. If the floor were the best score overall, a shape where
  everything scores badly would let the simplest bad candidate set its own pass
  mark.
- **The error budget scales with the shape's own size** (`scaled_budget`).
  Coverage error at a boundary is roughly the geometric displacement in pixels.
  Half a pixel is invisible on a 200px mark and is the entire shape on a 4px one.

### 8. Output — `svg.rs`

Primitives are emitted as real `<circle>`, `<ellipse>` and `<rect>` elements
rather than flattened to path data, because that is what makes the output
editable. Shapes with holes become even-odd paths. Path data omits repeated
command letters. Colours covering more of the canvas paint first, so detail lands
on top.

## Where to intervene

**Adding a new primitive** (rounded rectangle, capsule, regular polygon):

1. Write the fit in `primitive.rs`, returning its own worst-case error. Do not
   let it decide acceptance.
2. Add a variant to `Outline` in `path.rs`, and implement `to_contour`,
   `node_count`, `bounds` and `area` for it. `to_contour` is what the rasterizer
   scores, so it must be geometrically exact.
3. Offer it in `candidates_for` in `vectorize.rs`, behind a cheap algebraic gate
   so obviously-wrong candidates never cost a rasterization.
4. Emit it in `svg.rs`.
5. Add a ground-truth case to `tests/quality.rs` and to `examples/benchmark.rs`.

The selection machinery needs no changes. A new candidate wins only if it
measures at least as well while being simpler, which is exactly the property you
want.

**Changing a fitter** (curves, circles, corners): the module boundaries are real,
so you can usually replace one function and let the benchmark judge. The
tolerance ladder in `candidates_for` means a fitter that is better *at a given
tolerance* shows up as fewer nodes at equal accuracy, which the benchmark reports
directly.

**Adding a stage** (gradients, strokes): both of these need a new decision point
before candidate selection — a gradient region is not a flat-colour region, and a
stroked path is not a filled outline. Expect to touch `vectorize.rs`'s per-colour
loop and add a module, rather than to slot into an existing stage. Open an issue
and sketch the approach first; these are the two changes most likely to collide
with someone else's work.

**Making it faster:** the per-colour loop in `vectorize` is embarrassingly
parallel, and `field::decompose` is the hot loop. Note that parallelism must not
change output ordering — determinism is tested.

## Beyond the core

- `png2svg/cli/` — a thin `clap` wrapper. Option validation and exit codes live
  here; nothing about geometry does.
- `web-ui/` — React + TypeScript + Tailwind 3.4.x. The core is compiled to
  `wasm32-unknown-unknown` by `scripts/build-wasm.sh`, and runs in a Web Worker.
  The build drops the image decoders because the page decodes natively, which
  takes the payload from ~393KB to ~150KB gzipped. See `web-ui/README.md`.
- `benchmarks/` — the ground-truth benchmark (in CI) and the shootout against
  potrace and VTracer (not in CI; needs external tools). See
  `benchmarks/README.md`, which lists the caveats.
- `png2svg/core/tests/` — `quality.rs` asserts recovered geometry against known
  values; `logo_fixtures.rs` asserts behaviour through the `Document` API rather
  than through SVG text, so output formatting can change without breaking tests.

## Invariants

Break any of these and something downstream stops making sense.

1. **Output is deterministic.** Same input and options, byte-identical SVG.
2. **Coverage fields sum to 1.0 at every pixel.** The decomposition is a
   partition, not a set of independent masks.
3. **Positions are sub-pixel everywhere except crisp mode**, which is exactly
   on-grid by design.
4. **Primitives are hypotheses, never assumptions.** Nothing is emitted as a
   circle because it looked like one; it is emitted because rendering it back
   measured well.
5. **Nothing is decided by a guessed threshold.** Where a constant exists, it is
   a gate that avoids wasted work, not the thing that makes the decision.
