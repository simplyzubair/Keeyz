import { defineConfig } from 'vite';
import { resolve } from 'path';
import { readdirSync } from 'fs';

const pages = Object.fromEntries(
  readdirSync(resolve(__dirname, 'src/pages'))
    .filter((f) => f.endsWith('.html'))
    .map((f) => [f.replace('.html', ''), resolve(__dirname, 'src/pages', f)])
);

export default defineConfig({
  root: 'src/pages',
  publicDir: resolve(__dirname, 'public'),
  clearScreen: false,
  server: { port: 1430, strictPort: true },
  build: {
    outDir: resolve(__dirname, 'dist'),
    emptyOutDir: true,
    target: 'es2022',
    rollupOptions: { input: pages },
  },
});
