import { defineConfig } from '@playwright/test';
import { fileURLToPath } from 'node:url';

export default defineConfig({
  testDir: '.',
  testMatch: 'startup.spec.js',
  timeout: 90_000,
  expect: { timeout: 60_000 },
  workers: 1,
  retries: 0,
  reporter: 'list',
  outputDir: 'test-results',
  use: {
    browserName: 'chromium',
    baseURL: 'http://127.0.0.1:8768',
    viewport: { width: 1280, height: 800 },
    deviceScaleFactor: 1,
    headless: true,
    // Exercise the real renderer on CI hosts without a physical GPU.
    launchOptions: { args: ['--use-angle=swiftshader', '--enable-unsafe-swiftshader'] },
    trace: 'retain-on-failure',
  },
  webServer: {
    command: 'python3 -m http.server 8768 --bind 127.0.0.1 --directory dist/web',
    cwd: fileURLToPath(new URL('../../../', import.meta.url)),
    url: 'http://127.0.0.1:8768',
    reuseExistingServer: false,
    timeout: 30_000,
  },
});
