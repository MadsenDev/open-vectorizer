import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

/**
 * Vite wants a base with both a leading and a trailing slash.
 *
 * GitHub's configure-pages action reports `/<repo>` for a project site and plain
 * `/` for a user or organisation site, so normalising here keeps the workflow
 * from having to care which it is.
 */
function normalizeBase(value: string | undefined): string {
  const trimmed = value?.replace(/^\/+|\/+$/g, '') ?? '';
  return trimmed === '' ? '/' : `/${trimmed}/`;
}

// Defaults to the project-site path; `BASE_PATH=/ npm run build` for a root deploy.
const base = normalizeBase(process.env.BASE_PATH ?? '/open-vectorizer/');

export default defineConfig({
  base,
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
