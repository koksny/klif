import { defineConfig } from 'vite';
import { svelte } from '@sveltejs/vite-plugin-svelte';

// The dev-server port is registered in .studio/devserver.json. Never change it here alone.
export default defineConfig({
  plugins: [svelte()],
  server: {
    host: '127.0.0.1',
    port: 5193,
    strictPort: true,
  },
  preview: {
    host: '127.0.0.1',
    port: 5193,
    strictPort: true,
  },
  build: {
    target: 'es2022',
  },
});
