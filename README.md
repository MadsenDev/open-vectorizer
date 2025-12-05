# Open Vectorizer

Open Vectorizer is an in-progress, fully open-source PNG → SVG converter. The project aims to deliver a Rust core engine, a friendly CLI, and a Web UI powered by WebAssembly.

## Repository layout

- `Cargo.toml` – Rust workspace definition for the core engine and CLI.
- `png2svg/core/` – Core Rust library crate that will host the vectorization pipeline.
- `png2svg/cli/` – Command-line wrapper that calls the core engine.
- `web-ui/` – Placeholder for the upcoming React + TypeScript + Tailwind 3.4 front-end.

## Getting started

### Prerequisites
- Rust toolchain (edition 2021+)
- `cargo` available in your PATH

### Build and test

```bash
cargo test
```

### Run the CLI

```bash
cargo run -p png2svg-cli -- path/to/input.png --output output.svg \
  --colors 8 --detail 0.5 --smoothness 0.5 --tolerance 1.5 --mode logo
```

If `--output` is omitted, the SVG is printed to stdout. The current engine performs a lightweight palette reduction and emits merged `<rect>` rows to keep the SVG editable while we build out the full curve-based pipeline.

#### Options at a glance

- `--colors` (`2-64`, default `8`): palette size target after quantization. Increase for softer gradients, decrease for graphic shapes.
- `--detail` (`0.0-1.0`, default `0.5`): how much fine structure to preserve. Higher values keep more small regions.
- `--smoothness` (`0.0-1.0`, default `0.5`): softens edges; set lower to keep crisp pixel boundaries.
- `--tolerance` (`0.1-10.0`, default `1.5`): how aggressively nearby segments are merged. Larger values yield fewer, coarser shapes.
- `--mode` (`logo` | `poster` | `pixel`): presets for common asset types.

The CLI will reject out-of-range values with clear errors so you can quickly iterate on settings.

## Roadmap snapshot

See `PROJECT.md` for the high-level goals, including a WASM build and web experience.
