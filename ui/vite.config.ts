/// <reference types="vitest/config" />
import { fileURLToPath } from 'node:url';
import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';

const isVitest = process.env.VITEST !== undefined;

// Kelta UI (Svelte 5, no SvelteKit). Tauri loads `dist/` (tauri.conf.json frontendDist) and uses
// http://localhost:5173 as devUrl. `VITE_IPC=mock` swaps the Tauri IPC for the in-memory mock.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  resolve: {
    alias: {
      $lib: fileURLToPath(new URL('./src/lib', import.meta.url)),
      $app: fileURLToPath(new URL('./src/app', import.meta.url)),
    },
    // Svelte's browser build in vitest (component tests mount real components).
    conditions: isVitest ? ['browser'] : undefined,
  },
  server: {
    port: 5173,
    strictPort: true,
    host: '127.0.0.1',
  },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: ['safari16', 'chrome120'],
    outDir: 'dist',
    emptyOutDir: true,
    sourcemap: false,
    reportCompressedSize: true,
    chunkSizeWarningLimit: 600,
    rolldownOptions: {
      output: {
        // Vite 8 (rolldown) replacement of `manualChunks`: xterm core + eager addons in one chunk.
        // WebGL and search addons stay dynamic imports (their own chunks). Views are lazy through
        // `src/app/registry.ts`, so each view directory becomes its own chunk.
        codeSplitting: {
          groups: [
            {
              name: 'xterm',
              test: /node_modules[\\/](\.pnpm[\\/][^\\/]+[\\/]node_modules[\\/])?@xterm[\\/](xterm|addon-fit|addon-unicode11|addon-web-links)[\\/]/,
            },
          ],
        },
      },
    },
  },
  test: {
    environment: 'jsdom',
    include: ['src/**/*.test.ts', 'tests/unit/**/*.test.ts'],
    setupFiles: ['./tests/setup.ts'],
    // The first test of a file pays the cold transform of its components (seconds when the machine
    // is loaded); 5 s flaked. Real hangs still fail, only later.
    testTimeout: 30_000,
    restoreMocks: true,
    css: false,
  },
});
