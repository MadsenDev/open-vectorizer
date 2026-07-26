<!--
Thanks for contributing. CONTRIBUTING.md has the detail; the short version is
that changes here are judged by measurement, and accuracy and node count are
read together.
-->

## What this changes

<!-- And why. If it fixes an issue, say "Fixes #123". -->

## Benchmark

<!--
For anything touching the engine, run this before and after and paste both:

    cargo run --release -p png2svg-core --example benchmark

Accuracy and node count belong together: any vectorizer can buy accuracy by
emitting more geometry. If a case regressed, say so and explain the trade — a
known, explained regression is fine; a silent one is not.

If your change is outside what the synthetic benchmark covers (gradients,
strokes, real logos, the web UI, docs), delete this section and say what
evidence you do have instead.
-->

| case | before: accuracy / nodes | after: accuracy / nodes |
| --- | --- | --- |
|  |  |  |

## Checks

- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes
- [ ] Output is still deterministic (same input and options → byte-identical SVG)
- [ ] New behaviour has a test; ground-truth cases go in `png2svg/core/tests/quality.rs`
- [ ] No new dependency, or the PR explains why one is worth it

## Anything else

<!-- Trade-offs you made, alternatives you rejected, things you would like a second opinion on. -->
