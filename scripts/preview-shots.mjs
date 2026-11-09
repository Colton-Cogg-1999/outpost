// Renders the interface (with mocked native calls) in headless Chromium and saves
// screenshots. Development tool for layout work; it is not a test of the backend.
// Usage: node scripts/preview-shots.mjs <output-dir> [width height scale]
import { createServer } from 'vite';
import { chromium } from 'playwright-core';
import { mkdirSync } from 'node:fs';

const out = process.argv[2] ?? 'screenshots';
const width = Number(process.argv[3] ?? 1480);
const height = Number(process.argv[4] ?? 920);
const scale = Number(process.argv[5] ?? 1);
mkdirSync(out, { recursive: true });

const RUY = `[Event "Club night"]
[White "station_7"]
[Black "PolarNight"]
[Result "*"]

1. e4 e5 2. Nf3 Nc6 3. Bb5 a6 4. Ba4 Nf6 5. O-O Be7 6. Re1 b5 7. Bb3 O-O 8. c3 d6 *`;

const server = await createServer({ server: { port: 1430, strictPort: true }, logLevel: 'error' });
await server.listen();
const browser = await chromium.launch({ executablePath: process.env.CHROMIUM ?? '/usr/bin/chromium', headless: true });
const page = await browser.newPage({ viewport: { width, height }, deviceScaleFactor: scale });
const errors = [];
page.on('pageerror', (e) => errors.push(String(e)));
page.on('console', (m) => m.type() === 'error' && !m.text().startsWith('Failed to load resource') && errors.push(m.text()));
page.on('response', (r) => r.status() >= 400 && errors.push(`${r.status()} ${r.url()}`));
const tag = `${width}x${height}`;
const shot = async (name) => { await page.mouse.move(0, 0); await page.screenshot({ path: `${out}/${tag}-${name}.png` }); };
const store = (fn, arg) => page.evaluate(fn, arg);

await page.goto('http://127.0.0.1:1430/preview.html');
await page.waitForSelector('.board .piece');
await page.waitForTimeout(400);
await shot('01-start');

// Load a game, go to move 4…Nf6, run the engine.
await store((pgn) => window.__outpost.getState().loadPgn(pgn), RUY);
await store(() => {
  const s = window.__outpost.getState();
  for (let i = 0; i < 8; i++) s.next();
  s.setEngineOn(true);
  s.markSaved(1);
});
await page.waitForTimeout(1400);
await shot('02-engine');
// The simplified engine card must keep all existing controls reachable.
await page.getByRole('button', { name: 'Show more lines' }).click();
await page.getByRole('button', { name: 'More lines' }).click();
await page.waitForTimeout(500);
if (await store(() => window.__outpost.getState().prefs.multipv) !== 2) errors.push('MultiPV control did not update');
await shot('02b-engine-expanded');
await page.getByRole('button', { name: 'Fewer lines' }).click();
await page.getByRole('button', { name: 'Hide analysis details' }).click();

// The agent responds only to a typed question.
await page.getByLabel('Ask the AI assistant').fill('Explain the position.');
await page.getByLabel('Ask the AI assistant').press('Enter');
await page.waitForTimeout(1600);
await shot('03-ai');
if (!await page.locator('.ai-msg.assistant').count()) errors.push('Typed question did not produce an answer');
await page.getByLabel('Ask the AI assistant').fill('What is the main plan here?');
await page.getByLabel('Ask the AI assistant').press('Enter');
await page.waitForTimeout(1600);
if (await page.locator('.ai-msg.user').count() !== 2) errors.push('Agent composer did not submit with Enter');


// Review the game.
await page.getByLabel('Move list options', { exact: true }).click();
await page.click('.moves-panel .tabs button:nth-child(2)');
await page.getByLabel('Move list options', { exact: true }).click();
await page.click('text=Review game');
await page.waitForTimeout(1500);
await store(() => {
  const s = window.__outpost.getState();
  s.last();
});
await page.waitForTimeout(800);
await shot('04-review');
await page.getByRole('button', { name: 'Game review', exact: true }).click();
await store(() => {
  const s = window.__outpost.getState();
  s.first();
  for (let i = 0; i < 7; i++) s.next();
});
await page.waitForTimeout(900);
await shot('05-moves-classified');

// Drag a piece, draw an arrow.
await store(() => window.__outpost.getState().newGame());
await page.waitForTimeout(300);
await page.locator('.board').scrollIntoViewIfNeeded();
const box = await page.locator('.board').boundingBox();
const sq = (file, rank) => ({ x: box.x + (file + 0.5) * (box.width / 8), y: box.y + (7 - rank + 0.5) * (box.height / 8) });
let a = sq(4, 1), b = sq(4, 3);
await page.mouse.move(a.x, a.y); await page.mouse.down(); await page.mouse.move(b.x, b.y, { steps: 6 }); await page.mouse.up();
a = sq(6, 0); b = sq(5, 2);
await page.mouse.move(a.x, a.y); await page.mouse.down({ button: 'right' }); await page.mouse.move(b.x, b.y, { steps: 4 }); await page.mouse.up({ button: 'right' });
await page.waitForTimeout(700);
await shot('06-interaction');
const moved = await store(() => window.__outpost.getState().tree.nodes[window.__outpost.getState().current].san);
if (moved !== 'e4') errors.push(`drag-and-drop did not play e4 (got ${moved})`);
await page.getByLabel('Board tools', { exact: true }).click();
await page.getByLabel('Type a move in SAN or UCI').fill('e5');
await page.getByLabel('Type a move in SAN or UCI').press('Enter');
const typed = await store(() => window.__outpost.getState().tree.nodes[window.__outpost.getState().current].san);
if (typed !== 'e5') errors.push(`Board tools did not play e5 (got ${typed})`);
await page.getByLabel('Flip board', { exact: true }).click();
if (await store(() => window.__outpost.getState().orientation) !== 'black') errors.push('Board tools did not flip the board');
await page.getByLabel('Board tools', { exact: true }).click();


for (const [view, name] of [['games', '07-games'], ['tactics', '08-tactics'], ['engines', '09-engines'], ['ai', '10-ai'], ['settings', '11-settings']]) {
  await store((v) => window.__outpost.getState().setView(v), view);
  await page.waitForTimeout(view === 'tactics' ? 1200 : 400);
  await shot(name);
}
// A scan in progress, seen from the Tactics page and from elsewhere (sidebar).
await store(() => window.__outpost.getState().setScan({ job: 1, progress: { gameIndex: 2, games: 25, gameId: 3, label: 'station_7 – PolarNight', stage: 'review', done: 34, total: 81, found: 3, finished: false } }));
await store((v) => window.__outpost.getState().setView(v), 'tactics');
await page.waitForTimeout(400);
await shot('08b-tactics-scan');
await store((v) => window.__outpost.getState().setView(v), 'games');
await page.waitForTimeout(300);
await shot('08c-scan-sidebar');
await store(() => window.__outpost.getState().setScan(null));
await store(() => window.__outpost.getState().openImport('lichess'));
await page.waitForTimeout(300);
await shot('12-import');

await browser.close();
await server.close();
if (errors.length) {
  console.error('Console errors:\n' + errors.join('\n'));
  process.exitCode = 1;
} else console.log(`screenshots written to ${out}`);
