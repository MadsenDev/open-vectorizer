# Web UI

Vite + React + TypeScript front-end for Open Vectorizer, styled with Tailwind CSS
3.4.x. Everything runs client-side: the engine is compiled to WebAssembly and the
image never leaves the browser.

## Getting started

```bash
npm install
npm run dev
```

`npm run dev` and `npm run build` both build the WebAssembly bundle first, so
there is no separate step to remember. `npm run build:only` skips it when the
bundle is already current.

## How it fits together

- **`scripts/build-wasm.sh`** compiles `png2svg-core` for `wasm32-unknown-unknown`
  and runs `wasm-bindgen` into `src/pkg/` (generated, git-ignored). It reads the
  required `wasm-bindgen-cli` version from the workspace lockfile and installs it
  if the local one does not match, because a mismatch is a hard error.
- **The build drops the image decoders** (`--no-default-features`). The page
  decodes with `createImageBitmap`, so every format the browser reads is
  supported — WebP, AVIF and HEIC included, where available — and the payload
  falls from about 393KB to 150KB gzipped.
- **`src/vectorizer.worker.ts`** runs the conversion off the main thread. A
  1024px image takes a few hundred milliseconds and a 2048px one takes seconds;
  on the main thread that reads as a frozen tab.
- **Decoding and downscaling happen on the main thread**, where a 2D canvas is
  universally available. Each run sends the worker a copy of the pixels and
  transfers that copy, so the buffer survives for the next run when a slider
  moves — transferring the original would empty it after the first conversion.
- **Below `lg` the options become a bottom sheet**, reached from a floating
  button, so changing a setting on a phone does not mean scrolling back up. It is
  deliberately *not* modal and has no backdrop: the result has to stay visible and
  undimmed while a slider moves. Opening the sheet scrolls the result pane to the
  top and caps its height to the space left over, so the thing you are adjusting
  is fully on screen. It is one element, restyled by breakpoint, rather than two
  copies of the same controls.
- **Input above 2048px on the long edge is downscaled**, and the page says so.
  Coverage is held as one full-canvas field per palette colour, so memory grows
  as `width × height × colours`; 2048px with eight colours is about 150MB, and
  4096px would be four times that.

The UI reports node count and accuracy next to the result. Those belong together:
any vectorizer can buy accuracy by emitting more geometry.

## Deployment

Pushes to `main` build and publish to GitHub Pages via
`.github/workflows/pages.yml`, reachable at
[vector.vardir.no](https://vector.vardir.no) and at
`vardirhq.github.io/open-vectorizer`.

Asset paths are **relative**, so one artifact serves correctly from both: the
custom domain at the root and the project site under `/open-vectorizer/`. Both
layouts are covered by the smoke tests below.

The workflow used to compute an absolute base from the `configure-pages` output.
That does report the real serving path, so it worked — but it made every build
depend on CI resolving it, and a plain `npm run build` produced output that only
worked under the project-site prefix. Relative paths remove the coupling: the
workflow passes no base at all, and a local build is deployable anywhere.

Safe here only because this is a single page with no client-side routing.
`BASE_PATH=/some/prefix npm run build` forces an absolute base if that changes.

## Browser smoke tests

Two scripts, checking the things a unit test cannot see.

```bash
npm run build
npx vite preview --port 4180 &
npm i --no-save playwright

# Desktop: the wasm loads from its hashed asset path, the worker starts, and the
# page renders real SVG.
node scripts/smoke.mjs ../benchmarks/shootout/cases/ring.png

# Mobile (iPhone 13 viewport): the options sheet parks off-screen and leaves the
# tab order when closed, opening it keeps the result fully visible, sliders inside
# it still reach the engine, Escape closes it, and the floating button covers no
# content at the bottom of the page.
node scripts/smoke-mobile.mjs ../benchmarks/shootout/cases/badge.png
```

Both exit non-zero on any console error or failed assertion. `SCREENSHOT` and
`SCREENSHOT_DIR` capture the pages, `CHROMIUM` overrides the browser path and
`PREVIEW_URL` the address.
