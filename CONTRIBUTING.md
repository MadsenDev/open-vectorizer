# Contributing

Open Vectorizer already performs strongly on geometric flat artwork. It is not
finished. If you are interested in computational geometry, image processing,
SVG or Rust, there is a lot left to solve here and contributions and experiments
are very welcome.

The useful thing about this project is that it is **measurable**. There is a
benchmark against known geometry, and a shootout against potrace and VTracer on
identical inputs, scored by an external renderer. So you do not have to argue
that your ellipse fitter is better — you can show it. That also means you do not
have to understand the whole engine to contribute. Most of the open problems sit
inside one module.

## Where the interesting problems are

Each of these is real, currently unsolved, and bounded. Difficulty is a rough
guide, not a gate.

| Problem | Where it lives | Difficulty |
| --- | --- | --- |
| **Gradient detection** — recognise a banded region as one `<linearGradient>` / `<radialGradient>` | `quantize.rs`, `field.rs`, `svg.rs` | hard |
| **Stroke recovery** — spot that a filled outline was originally a stroked path, and emit `stroke-width` | new module + `vectorize.rs` | hard |
| **Rounded-rectangle primitive** — `<rect rx>` instead of 4 lines + 4 curves | `primitive.rs`, `vectorize.rs` | moderate |
| **Better pixel-art detection** — the current test is size-and-alpha; look for an integer upscale grid instead | `vectorize.rs::resolve_auto_options` | moderate |
| **Speed** — we are 2–4× slower than VTracer. The per-colour loop is embarrassingly parallel and the coverage decomposition is the hot loop | `vectorize.rs`, `field.rs` | moderate |
| **Relative flatness in `interior_mask`** — a low-contrast boundary steps by less than the fixed threshold, so its blend colours reach the palette | `quantize.rs:112` | moderate |
| **A corpus of real logos** with committed expected outputs — glyphs, thin strokes, drop shadows, recompressed screenshots | `benchmarks/`, `tests/logo_fixtures.rs` | no geometry needed |
| **More benchmark competitors** — anything runnable and non-commercial | `benchmarks/shootout/run.sh` | no geometry needed |
| **Curve fitting** — `fit.rs` is Schneider with Newton-Raphson reparameterization. There are better schemes | `fit.rs` | moderate |
| **Batch mode** — convert a directory | `png2svg/cli/src/main.rs` | easy |
| **Web UI: example gallery, side-by-side zoom and pan** | `web-ui/` | easy, front-end |
| **Raise the 2048px input ceiling** — stream coverage per colour instead of holding every field at once | `field.rs`, `vectorize.rs` | moderate |

Issues labelled [`good first issue`](https://github.com/vardirhq/open-vectorizer/labels/good%20first%20issue)
and [`help wanted`](https://github.com/vardirhq/open-vectorizer/labels/help%20wanted)
are the ones with the clearest edges.

**Alternative algorithms are explicitly welcome.** If you think the ellipse
fitter should be a direct conic fit rather than area-moment matching, or that
corner detection should be curvature scale-space rather than the two-window turn
ratio, the benchmark will settle it. Do not treat the current implementation as
the specification. Several of these choices were made because they were the
simplest thing that measured well, not because they are the best available.

`TODO.md` is the full checklist, including what is already done.
`docs/ARCHITECTURE.md` explains how the pipeline fits together and what each
stage guarantees the next one.

## Getting set up

You need a Rust toolchain (edition 2021+). Nothing else, for the core.

```bash
cargo test --workspace     # the whole suite, including ground-truth quality tests
cargo clippy --workspace --all-targets -- -D warnings
```

Both run in CI, and clippy warnings are errors there.

```bash
# Measure against geometry whose exact values are known.
cargo run --release -p png2svg-core --example benchmark

# Write sample PNG inputs and SVG outputs to target/vectorizer-samples/.
cargo run --release -p png2svg-core --example generate_samples

# Convert something.
cargo run --release -p png2svg-cli -- logo.png -o logo.svg --stats
```

For the web UI (`web-ui/README.md` has the detail):

```bash
cd web-ui
npm install
npm run dev
```

For the shootout against potrace and VTracer, which needs external tools and so
is not in CI, see `benchmarks/README.md`.

## The rule: measure it

This engine's central idea is that geometry is chosen by measurement rather than
by a tuned threshold. Contributions are held to the same standard.

**Node count and accuracy belong together.** Any vectorizer can buy accuracy by
emitting more geometry. A change that improves accuracy while emitting more nodes
has not obviously improved anything, and a PR that reports only one of the two
numbers cannot be evaluated.

So: run the benchmark before and after your change, and put both tables in the
PR.

```bash
git stash                                                              # or check out main
cargo run --release -p png2svg-core --example benchmark > /tmp/before.txt
git stash pop
cargo run --release -p png2svg-core --example benchmark > /tmp/after.txt
diff /tmp/before.txt /tmp/after.txt
```

If your change touches something the synthetic benchmark does not cover —
gradients, strokes, real-world logos — say so plainly and show what evidence you
do have. "The benchmark is unchanged, and here is the input it does not cover"
is a perfectly good result. Quietly regressing a benchmark case is not.

If you add a capability, add a case that exercises it. `png2svg/core/tests/quality.rs`
renders known geometry, vectorizes it, and asserts the original geometry came
back; that is the pattern to follow.

## House style

- **Determinism is a hard requirement.** The same input and options must always
  produce byte-identical SVG. That means no iteration over `HashMap`, no
  unseeded randomness, and stable sorts where ties are possible. There are tests
  for this.
- **Comments explain why, not what.** The existing comments record the reasoning
  behind a choice — usually the failure that motivated it. If you change a
  decision, change the comment that justifies it. If you make a non-obvious
  choice, leave a note saying what goes wrong otherwise.
- **British spelling in prose, American in code.** `colour` in a sentence,
  `color` in an identifier. This is what the codebase already does; it is not
  worth changing either way.
- **New dependencies need a reason.** The wasm payload is 150KB gzipped and that
  is a feature. A dependency that costs 50KB to save 30 lines is a bad trade.
- **Keep the public API honest.** `png2svg-core` exposes its stages as modules on
  purpose, so they can be tested and swapped individually.
- **No `unsafe`** in the core without a comment explaining why it is necessary
  and why it is sound.

## Sending the change

1. Fork, and branch from `main`.
2. Keep the change to one thing. A faster decomposition and a new primitive are
   two PRs.
3. Make sure `cargo test --workspace` and `cargo clippy --workspace --all-targets
   -- -D warnings` both pass.
4. Write the commit message in the imperative and say *why*, not just what
   ("Recover rounded rectangles as `<rect rx>`, not four curves").
5. Open the PR. The template asks for the before/after benchmark; fill it in.

Small fixes — typos, a clearly wrong comment, an obvious bug — do not need an
issue first. For anything that changes the shape of the output or the pipeline,
open an issue and describe the approach before writing a lot of code. Not for
permission: so that someone can tell you if that corner of the engine is about
to change underneath you.

## Things that will get pushed back on

Not to be discouraging — these are just the recurring ones, and it is cheaper to
say so here:

- A tuning change that improves one benchmark case and quietly degrades three.
- A new magic threshold. If a value has to be guessed, the design is probably
  wrong; the engine's whole approach is to generate candidates and measure them.
- Accuracy bought with nodes, reported as an accuracy win.
- Photograph support. It is a stated non-goal — a photograph vectorizes into
  thousands of shapes, which is the wrong representation. See "Scope" in the
  README.
- A learned model. Considered and declined, for reasons in `PROJECT.md`: it
  would cost determinism and the offline guarantee, and it would have to beat
  these numbers on this benchmark to be worth it.
- Reformatting unrelated code in the same PR.

## Reporting a bug

An input that vectorizes badly is a genuinely useful bug report, and often more
useful than a feature request. Attach the PNG, the exact command, and what you
expected. `--stats` output helps. The issue templates ask for this.

## Code of conduct

By participating you agree to abide by [`CODE_OF_CONDUCT.md`](CODE_OF_CONDUCT.md).

## Licence

Contributions are made under the MIT licence, the same as the project. See
[`LICENSE`](LICENSE).
