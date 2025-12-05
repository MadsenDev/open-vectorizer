# Web UI

Vite + React + TypeScript front-end for Open Vectorizer. Tailwind CSS 3.4.x is used for styling.

## Getting started

```bash
npm install
npm run dev
```

The dev server runs at http://localhost:5173 by default.

Before running the UI, build the WebAssembly bundle from the Rust core (requires `wasm-pack`):

```bash
wasm-pack build ../png2svg/core --target web --out-dir public/pkg --release
```

## Notes

- The preview calls the WASM-exported `png_to_svg_wasm` function once the bundle in `public/pkg` is present; otherwise it
  falls back to a lightweight placeholder.
- Presets mirror the CLI options so values can flow directly into the vectorizer core.
