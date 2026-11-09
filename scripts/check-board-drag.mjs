// Real-pointer regression check for the scaled board and its viewport drag image.
// Usage: node scripts/check-board-drag.mjs [--webkit]
import assert from 'node:assert/strict';
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { createServer } from 'vite';
import { chromium } from 'playwright-core';
const server = await createServer({ server: { port: 1431, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM ?? '/usr/bin/chromium', headless: true });
try {
  for (const width of [1094, 1448]) {
    const page = await browser.newPage({ viewport: { width, height: width === 1094 ? 688 : 1038 }, deviceScaleFactor: 2 });
    await page.goto('http://127.0.0.1:1431/preview.html');
    await page.waitForSelector('.board .piece');
    await page.evaluate(() => window.__outpost.getState().setEngineOn(false));
    for (const flip of [false, true]) {
      await page.evaluate((flip) => {
        const s = window.__outpost.getState();
        s.newGame();
        if ((s.orientation === 'black') !== flip) s.flip();
      }, flip);
      const board = await page.locator('.board').boundingBox();
      const at = (name) => {
        const file = name.charCodeAt(0) - 97, rank = Number(name[1]) - 1;
        return { x: board.x + ((flip ? 7 - file : file) + .5) * board.width / 8,
          y: board.y + ((flip ? rank : 7 - rank) + .5) * board.height / 8 };
      };
      const start = at('e2'), end = at('e4');
      await page.mouse.move(start.x, start.y);
      await page.mouse.down();
      // Every event moves less than a pixel; the threshold must use the pickup point.
      await page.mouse.move(end.x, end.y, { steps: 150 });
      const ghost = await page.locator('.drag-ghost').boundingBox();
      assert(ghost, 'Slow drag must produce a visible piece');
      assert(Math.abs(ghost.x + ghost.width / 2 - end.x) < .5, 'Dragged piece must track pointer X');
      assert(Math.abs(ghost.y + ghost.height / 2 - end.y) < .5, 'Dragged piece must track pointer Y');
      assert(Math.abs(ghost.width - board.width / 8) < .5, 'Dragged piece must retain square size');
      await page.mouse.up();
      assert.equal(await page.evaluate(() => { const s = window.__outpost.getState(); return s.tree.nodes[s.current].uci; }), 'e2e4');
      assert.equal(await page.locator('.drag-ghost').count(), 0);

      await page.evaluate((flip) => { const s = window.__outpost.getState(); s.newGame(); if (flip) s.flip(); }, flip);
      await page.mouse.move(start.x, start.y);
      await page.mouse.down();
      await page.mouse.move(board.x - 20, board.y - 20, { steps: 8 });
      await page.mouse.up();
      assert.equal(await page.locator('.drag-ghost').count(), 0, 'Outside drop must remove the dragged piece');
      assert.equal(await page.evaluate(() => { const s = window.__outpost.getState(); return s.current === s.tree.root; }), true);

      // Leaving the board and returning before release must retain capture.
      await page.mouse.move(start.x, start.y);
      await page.mouse.down();
      await page.mouse.move(board.x - 20, board.y - 20, { steps: 8 });
      await page.mouse.move(end.x, end.y, { steps: 8 });
      await page.mouse.up();
      assert.equal(await page.evaluate(() => { const s = window.__outpost.getState(); return s.tree.nodes[s.current].uci; }), 'e2e4');
      console.log(`${width}px, ${flip ? 'black' : 'white'} orientation: pointer alignment, size, slow drag, outside drop, re-entry passed`);
    }
    await page.close();
  }
  if (process.argv.includes('--webkit')) {
    const probe = spawn('/usr/bin/python', [fileURLToPath(new URL('./check-board-drag-webkit.py', import.meta.url))], {
      stdio: 'inherit', env: { ...process.env, PYTHONWARNINGS: 'ignore::DeprecationWarning' },
    });
    const code = await new Promise((resolve, reject) => { probe.on('error', reject); probe.on('exit', resolve); });
    assert.equal(code, 0, 'Native WebKit drag/drop check');
  }
} finally {
  await browser.close();
  await server.close();
}
