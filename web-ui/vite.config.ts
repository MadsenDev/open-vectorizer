import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

/**
 * Asset paths are relative by default, so one build works wherever it is served.
 *
 * This page is deployed at two URLs — a GitHub Pages project site under
 * `/open-vectorizer/` and a custom domain at the root — and a relative base
 * resolves correctly at both without the build having to know which. It also
 * means changing or adding a domain never silently 404s every asset.
 *
 * Safe here because this is a single page with no client-side routing; relative
 * paths would break for an app that served nested URLs.
 *
 * `BASE_PATH=/some/prefix` forces an absolute base if one is ever needed.
 */
function resolveBase(value: string | undefined): string {
  if (value === undefined) return './';
  const trimmed = value.replace(/^\/+|\/+$/g, '');
  return trimmed === '' ? '/' : `/${trimmed}/`;
}

export default defineConfig({
  base: resolveBase(process.env.BASE_PATH),
  plugins: [react()],
  worker: {
    // The vectorizer worker imports the wasm glue as an ES module.
    format: 'es',
  },
  build: {
    // Keep the wasm a separate file. Inlining it as a base64 data URL would bloat
    // the bundle and give up streaming compilation.
    assetsInlineLimit: 0,
  },
});
