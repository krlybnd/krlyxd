import { defineConfig } from '@playwright/test';
import base from '../../e2e/expert.config.ts';
export default defineConfig({
  ...base, testDir: '.', testMatch: 'cad.spec.ts', outputDir: '../../test-results/cad-tree',
  webServer: { command: 'npm exec vite -- preview --config web/vite.config.ts --host 127.0.0.1 --port 4329 --strictPort', cwd: '../..', url: 'http://127.0.0.1:4329', reuseExistingServer: false, timeout: 20_000 },
  use: { ...base.use, baseURL: 'http://127.0.0.1:4329' },
});
