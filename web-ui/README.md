# Web UI

Vite + React + TypeScript front-end for Open Vectorizer. Tailwind CSS 3.4.x is used for styling.

## Getting started

```bash
npm install
npm run dev
```

The dev server runs at http://localhost:5173 by default.

## Notes

- The preview currently uses a placeholder SVG generator. Once the Rust core is compiled to WebAssembly, wire the exported
  `png_to_svg_wasm` entry point into the existing controls and preview area.
- Presets mirror the CLI options so values can flow directly into the vectorizer core.
