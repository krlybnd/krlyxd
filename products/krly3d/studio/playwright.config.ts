import { existsSync } from 'node:fs';
import { defineConfig, devices } from '@playwright/test';

// Use an installed browser; never download one or start/restart either server.
const executablePath = process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH ?? [
  '/usr/bin/chromium', '/usr/bin/chromium-browser', '/snap/bin/chromium',
  '/usr/bin/google-chrome',
].find(path => existsSync(path));

export default defineConfig({
  testDir: './e2e',
  outputDir: './test-results',
  fullyParallel: false,
  workers: 1, // The real service has two native computation slots.
  retries: 0,
  forbidOnly: true,
  timeout: 90_000,
  expect: { timeout: 15_000 },
  reporter: 'list',
  use: {
    baseURL: process.env.STUDIO_UI_URL ?? 'http://127.0.0.1:4310',
    headless: true,
    acceptDownloads: true,
    actionTimeout: 15_000,
    navigationTimeout: 20_000,
    screenshot: 'only-on-failure',
    trace: 'retain-on-failure',
    launchOptions: {
      executablePath,
      // Exercise Three.js/WebGL even on machines without a physical GPU.
      args: ['--enable-unsafe-swiftshader'],
    },
  },
  projects: [
    { name: 'desktop-chromium', testMatch: 'studio.spec.ts', use: { viewport: { width: 1440, height: 1000 } } },
    { name: 'mobile-chromium', testMatch: 'mobile.spec.ts', use: { ...devices['Pixel 7'] } },
  ],
});
