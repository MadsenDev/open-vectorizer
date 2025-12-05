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

## Roadmap snapshot

See `PROJECT.md` for the high-level goals, including a WASM build and web experience.
