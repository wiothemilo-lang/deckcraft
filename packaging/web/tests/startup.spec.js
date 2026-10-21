import { expect, test } from '@playwright/test';

const routes = [
  ['sample', '/'],
  ['blank', '/?blank'],
  ['sample-webgl', '/?webgl'],
  ['blank-webgl', '/?blank&webgl'],
];

for (const [name, route] of routes) {
  test(`${name}: initializes a document and renders without a panic`, async ({ page }, testInfo) => {
    const errors = [];
    const log = [];
    page.on('pageerror', error => errors.push(error.message));
    page.on('crash', () => errors.push('browser page crashed'));
    page.on('console', message => {
      const text = message.text();
      log.push(`${message.type()}: ${text}`);
      if (message.type() === 'error' || /panicked at|no pids on this platform|sample deck:|new presentation:/i.test(text)) {
        errors.push(text);
      }
    });

    try {
      await page.goto(route, { waitUntil: 'domcontentloaded' });
      const canvas = page.locator('#deckcraft_canvas');
      await expect(canvas).toBeVisible();
      // A compile-only WASM check misses std::process::id() panicking during DocState::new.
      // Both sample and blank routes create a document before this overlay is removed.
      await expect(page.locator('#deckcraft_loading')).toHaveCount(0);
      await page.waitForFunction(() => {
        const canvas = document.getElementById('deckcraft_canvas');
        return canvas instanceof HTMLCanvasElement && canvas.width > 300 && canvas.height > 150;
      });
      await page.evaluate(() => new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve))));
      expect(errors, log.join('\n')).toEqual([]);
      expect(log.some(line => line.includes('wgpu backend')), log.join('\n')).toBe(true);
      if (name.endsWith('webgl')) {
        expect(log.some(line => /wgpu backend Gl\b/.test(line)), log.join('\n')).toBe(true);
      }
    } finally {
      await testInfo.attach('browser-console', { body: log.concat(errors).join('\n'), contentType: 'text/plain' });
      if (!page.isClosed()) await page.screenshot({ path: testInfo.outputPath('startup.png') });
    }
  });
}
