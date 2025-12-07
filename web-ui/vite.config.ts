import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [react()],
  optimizeDeps: {
    exclude: ['/pkg/png2svg_core.js'],
  },
  server: {
    fs: {
      // Allow serving files from the project root
      allow: ['..'],
    },
  },
});
