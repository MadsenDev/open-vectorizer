# Benchmarks

Two different things live here, and they answer different questions.

**`cargo run --release -p png2svg-core --example benchmark`** — measures Open
Vectorizer against geometry whose exact values are known. Did the circle come
back as a circle, with the right radius, to within a tenth of a pixel? This runs
in CI and needs nothing installed.

**`benchmarks/shootout/`** — compares Open Vectorizer against other engines on
the same inputs. Needs external tools, so it is not part of CI.

## Running the shootout

```bash
# Competitors and an SVG renderer for scoring.
apt-get install -y potrace librsvg2-bin
cargo install vtracer

cd benchmarks/shootout
cargo build --release
./target/release/gen ./cases
./run.sh
```

If `vtracer` is not on `PATH`, set `VTRACER=/path/to/vtracer`.

## Rebuilding the README figure

The comparison at the top of the README is generated from the same case
directory, after `run.sh` has produced each engine's output:

```bash
cd benchmarks/shootout
./target/release/figure ./cases ./figure
rsvg-convert -w 1684 -o ../../docs/images/comparison.png ./figure/comparison.svg
pngquant --quality 70-95 --speed 1 -f -o ../../docs/images/comparison.png \
  ../../docs/images/comparison.png
```

Node markers come from the same parser as the tables, so the dots in the picture
and the numbers underneath it cannot drift apart. The figure leaves out the
background rectangle each engine emits on an opaque input — it is not part of
the mark, and the rule is applied to every engine identically — while the tables
count the whole file.

## What it measures

Each tool is given the input its design expects, and every result is then scored
identically: render the SVG back with `rsvg-convert` and compare pixels against
the source raster. Nothing in the scoring uses Open Vectorizer's internals, so
the comparison cannot quietly favour our own model of the image.

- **accuracy** — `1 −` mean absolute per-pixel difference, in premultiplied
  alpha. Table 1 compares coverage only; Table 2 compares full RGBA.
- **nodes** — on-curve nodes, parsed from the SVG. `<circle>`, `<ellipse>` and
  `<rect>` count as one node each, which is the point of emitting them. Two
  things make this more than counting command letters. SVG allows implicit
  repetition, so a `C` followed by twelve numbers is two cubics under one letter
  and letter-counting reports half the geometry. And generators disagree about
  how to close a shape: potrace ends its last curve on the starting point and
  then writes `z`, while we write `Z` and let it draw the closing edge. Counting
  drawing segments therefore charges potrace for a node it does not have and
  lets us off one we do, so the parser collects the actual on-curve points of
  each subpath and drops a final point that merely repeats the start. A
  quadrilateral has four nodes either way it is written.
- **area** — rendered coverage minus source coverage, in pixels. This catches
  systematic dilation or erosion that an averaged accuracy figure hides.

Accuracy and node count belong together. Any vectorizer can buy accuracy by
emitting more geometry; a tool that wins on accuracy while emitting ten times the
nodes has not won.

**Table 1 (geometry)** uses the single-colour cases and compares coverage alone,
so potrace is not penalised for filling black rather than the source colour. Ours
reads the anti-aliased PNG; potrace gets the ideal thresholded bilevel mask, which
is the best input a 1-bit tracer can accept.

**Table 2 (colour)** puts every case on an opaque white background. That is the
fair common ground: VTracer is built for opaque input and traces the transparent
region as a shape otherwise, and "logo on white" is the commonest real input.
potrace is excluded because it cannot represent colour at all.

## Caveats

Read these before quoting any number.

- **The cases are our own synthetic geometry**, and they are biased toward what
  this engine was built for: primitives, polygons, rounded rectangles. Real logos
  also contain glyphs, thin strokes, gradients, drop shadows and recompressed
  screenshots, none of which are here. An engine that handles those better would
  score worse on this benchmark. This is the largest caveat by far, and the
  remedy is a corpus of real logos — tracked in `TODO.md`.
- **potrace is being used outside its design.** It is a 1-bit tracer for scanned
  line art. Feeding it anti-aliased artwork requires thresholding, which is the
  standard workflow but is exactly the scenario that exposes its 1-bit nature.
  Its curve fitting is excellent: on `blob`, the one case with no primitive to
  exploit, it is within a node of us.
- **Defaults only.** No tool was tuned. VTracer has `--mode`,
  `--corner_threshold`, `--filter_speckle` and presets; potrace has `--alphamax`
  and `--opttolerance`. Tuned settings would move the numbers.
- **Not every engine is here.** Inkscape's trace is potrace, so it is covered.
  Adobe Illustrator's Image Trace and Vector Magic are commercial and could not
  be run. AutoTrace is effectively unmaintained.
