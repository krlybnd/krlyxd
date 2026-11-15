import { fileURLToPath } from 'node:url';
import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

export default defineConfig({
  root: fileURLToPath(new URL('.', import.meta.url)),
  plugins: [react()],
  server: {
    host: '127.0.0.1',
    port: 4310,
    strictPort: true,
    proxy: { '/krly.studio.v1.StudioService': 'http://127.0.0.1:4311' },
  },
  build: {
    outDir: '../dist',
    emptyOutDir: true,
    rolldownOptions: {
      output: { codeSplitting: { groups: [{ name: 'three', test: /node_modules\/three\// }] } },
    },
  },
});
