import { existsSync } from 'node:fs';
import { defineConfig, devices } from '@playwright/test';

// Isolated, deterministic UI suite. Every RPC is intercepted; no service or
// provider credentials are needed. Run after npm run build:
// npx playwright test --config e2e/expert.config.ts
export default defineConfig({
  testDir: '.', testMatch: ['expert.spec.ts', 'character.spec.ts'], outputDir: '../test-results/expert',
  workers: 1, retries: 0, timeout: 30_000, forbidOnly: true,
  expect: { timeout: 7000 }, reporter: 'list',
  webServer: {
    command: 'npm exec vite -- preview --config web/vite.config.ts --host 127.0.0.1 --port 4328 --strictPort',
    cwd: '..', url: 'http://127.0.0.1:4328', reuseExistingServer: false, timeout: 20_000,
  },
  use: {
    baseURL: 'http://127.0.0.1:4328', headless: true, actionTimeout: 7000,
    launchOptions: {
      executablePath: ['/usr/bin/chromium', '/usr/bin/chromium-browser', '/snap/bin/chromium', '/usr/bin/google-chrome'].find(existsSync),
      args: ['--enable-unsafe-swiftshader'],
    },
    screenshot: 'only-on-failure', trace: 'retain-on-failure',
  },
  projects: [
    { name: 'expert-desktop', use: { viewport: { width: 1440, height: 1000 } } },
    { name: 'expert-mobile', use: { ...devices['Pixel 7'] } },
  ],
});
