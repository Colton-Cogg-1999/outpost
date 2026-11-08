// Self-test: drives the real interface inside the real (hidden) app window and
// writes what it sees to the report file named by OUTPOST_SELFTEST. Inert otherwise.
import { invoke, inTauri, errorText } from '../lib/ipc';
import { useStore, type View } from '../store';
import { startScan } from '../tactics/scan';

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

async function until(test: () => boolean, ms: number, step = 250) {
  const end = Date.now() + ms;
  while (Date.now() < end) {
    if (test()) return true;
    await wait(step);
  }
  return test();
}

function setInput(el: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(el, value);
  el.dispatchEvent(new Event('input', { bubbles: true }));
}

function button(label: RegExp, root: ParentNode = document) {
  return [...root.querySelectorAll('button')].find((b) => label.test(b.textContent ?? '')) as HTMLButtonElement | undefined;
}

const text = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();

export async function runSelftest() {
  if (!inTauri) return;
  const cfg = await invoke<{ user: string | null } | null>('selftest_info').catch(() => null);
  if (!cfg) return;
  const log = (line: string) => invoke('selftest_log', { line }).catch(() => {});
  const failures: string[] = [];
  const check = (ok: boolean, what: string) => {
    log(`${ok ? 'PASS' : 'FAIL'} ${what}`);
    if (!ok) failures.push(what);
  };
  window.addEventListener('error', (e) => log(`JS-ERROR ${e.message} @ ${e.filename}:${e.lineno}`));
  window.addEventListener('unhandledrejection', (e) => log(`JS-REJECT ${errorText(e.reason)}`));
  log(`START ${navigator.userAgent} ${window.innerWidth}x${window.innerHeight}`);
  try {
    await until(() => !!document.querySelector('.sidebar'), 10000);
    check(!!document.querySelector('.board'), 'analyze view renders a board');
    for (const v of ['analyze', 'games', 'tactics', 'engines', 'ai', 'settings'] as View[]) {
      useStore.getState().setView(v);
      await wait(700);
      const main = document.querySelector('main');
      log(`VIEW ${v}: ${text(main).slice(0, 300)}`);
      check(text(main).length > 20, `view ${v} has content`);
    }
    const toasts = () => text(document.querySelector('.toasts'));
    if (cfg.user) {
      const rowCount = () => document.querySelectorAll('main table tbody tr').length;
      let before = 0;
      for (const platform of ['lichess', 'chesscom'] as const) {
        // Start from the Games page, the way a person would, so a list that fails to refresh is caught.
        useStore.getState().setView('games');
        await wait(800);
        before = rowCount();
        useStore.getState().openImport(platform);
        await until(() => !!document.querySelector('.modal input[type=text]'), 3000);
        const input = document.querySelector('.modal input[type=text]') as HTMLInputElement | null;
        check(!!input, `${platform}: username field present`);
        if (!input) continue;
        setInput(input, cfg.user);
        const count = document.querySelector('.modal input[type=number]') as HTMLInputElement | null;
        if (count) setInput(count, '10');
        await wait(200);
        const go = button(/Import games/, document.querySelector('.modal')!);
        check(!!go && !go.disabled, `${platform}: import button enabled`);
        go?.click();
        const finished = await until(() => /Imported \d+|Stopped|error|no player/i.test(text(document.querySelector('.modal .notice')) + toasts()), 90000, 500);
        log(`${platform} notice: ${text(document.querySelector('.modal .notice'))} | toasts: ${toasts()}`);
        check(finished && /Imported \d+/.test(text(document.querySelector('.modal .notice'))), `${platform}: import reports completion`);
        await wait(800);
        log(`${platform}: list behind the dialog went ${before} -> ${rowCount()} rows`);
        check(rowCount() > before, `${platform}: open games list refreshes during import`);
        const open = button(/Open library/, document.querySelector('.modal')!);
        check(!!open, `${platform}: "Open library" offered`);
        open?.click();
        await wait(1200);
        const rows = document.querySelectorAll('main table tbody tr');
        log(`${platform} games view: ${rows.length} rows; first: ${text(rows[0] ?? null).slice(0, 160)}`);
        check(rows.length > before, `${platform}: new games listed after import`);
      }
      const accounts = text(document.querySelector('main')).match(/grinch|sync/i);
      log(`accounts section: ${accounts ? 'present' : 'absent'}`);
      const first = document.querySelector('main table tbody tr') as HTMLElement | null;
      first?.click();
      await wait(1500);
      const st = useStore.getState();
      const moves = st.tree.nodes[st.tree.root].children.length > 0;
      log(`after opening a game: view=${st.view} hasMoves=${moves} gameId=${st.gameId}`);
      check(st.view === 'analyze' && moves, 'opening an imported game loads it on the board');
    }
    // Engine and tactics: the built-in engine must be registered; scan two games end to end.
    const engines = await invoke<{ name: string; builtin: boolean; default: boolean }[]>('engines_list');
    log(`engines: ${engines.map((e) => `${e.name}${e.builtin ? ' [built in]' : ''}${e.default ? ' [default]' : ''}`).join(', ') || 'none'}`);
    check(engines.some((e) => e.builtin && e.default), 'built-in engine registered as default');
    if (cfg.user && engines.length) {
      useStore.getState().setView('tactics');
      await wait(800);
      const t0 = Date.now();
      await startScan({ strictness: 'personal', onlyMine: true, rescan: false, maxGames: 2 });
      const st = await invoke<{ scanned: number; tactics: number }>('tactics_stats');
      log(`tactic scan: ${st.scanned} game(s) scanned, ${st.tactics} tactic(s), ${((Date.now() - t0) / 1000).toFixed(1)}s | toasts: ${toasts()}`);
      check(st.scanned === 2, 'tactic scan completes two games');
      await wait(1500);
      log(`tactics view: ${text(document.querySelector('main')).slice(0, 240)}`);
      if (st.tactics > 0) check(!!document.querySelector('.puzzle-status'), 'a tactic is shown for training');
    }
    log(`TOASTS ${toasts()}`);
  } catch (e) {
    log(`CRASH ${errorText(e)}`);
    failures.push('crash');
  }
  log(failures.length ? `RESULT FAIL (${failures.length}): ${failures.join('; ')}` : 'RESULT PASS');
  await wait(300);
  await invoke('selftest_done').catch(() => {});
}
