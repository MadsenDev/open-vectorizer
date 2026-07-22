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
  --mode auto
```

If `--output` is omitted, the SVG is printed to stdout. The default `auto` mode inspects the image and chooses a logo, poster, or pixel-art vectorization strategy before tracing paths.

#### Options at a glance

- `--mode` (`auto` | `logo` | `poster` | `pixel`, default `auto`): automatic detection by default, with manual overrides for advanced use.
- `--colors` (`2-64`, default `8`): palette size target after quantization. In `auto` mode this is inferred from the input.
- `--detail` (`0.0-1.0`, default `0.5`): how much fine structure to preserve. In `auto` mode this is inferred from the input.
- `--smoothness` (`0.0-1.0`, default `0.5`): softens edges; set lower to keep crisp pixel boundaries. In `auto` mode this is inferred from the input.
- `--tolerance` (`0.1-10.0`, default `1.5`): how aggressively nearby segments are merged. In `auto` mode this is inferred from the input.

The CLI will reject out-of-range values with clear errors when you use manual overrides.

## Roadmap snapshot

See `PROJECT.md` for the high-level goals, including a WASM build and web experience.
