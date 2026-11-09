// Opt-in native integration check. Run with OUTPOST_DATA_DIR set to an isolated
// directory, OUTPOST_SELFTEST=<absolute log>, OUTPOST_SELFTEST_USER=<account>.
import { api, invoke, inTauri, errorText } from '../lib/ipc';
import { useStore, type View } from '../store';
import { useGameChat } from '../views/gameChat';
import { playUci } from '../chess/position';

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));
async function until(test: () => boolean, ms: number) {
  const end = Date.now() + ms;
  while (Date.now() < end) { if (test()) return true; await wait(200); }
  return test();
}
function setInput(el: HTMLInputElement, value: string) {
  Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value')!.set!.call(el, value);
  el.dispatchEvent(new Event('input', { bubbles: true }));
}
function button(label: RegExp, root: ParentNode = document) {
  return [...root.querySelectorAll('button')].find((b) => label.test(b.textContent ?? ''));
}
const text = (el: Element | null) => (el?.textContent ?? '').replace(/\s+/g, ' ').trim();

export async function runSelftest() {
  if (!inTauri) return;
  const cfg = await invoke<{ user: string | null; canClear: boolean } | null>('selftest_info').catch(() => null);
  if (!cfg) return;
  const log = (line: string) => invoke('selftest_log', { line });
  const check = async (ok: boolean, what: string) => {
    await log(`${ok ? 'PASS' : 'FAIL'} ${what}`);
    if (!ok) throw new Error(what);
  };
  window.addEventListener('error', (e) => void log(`JS-ERROR ${e.message}`));
  window.addEventListener('unhandledrejection', (e) => void log(`JS-REJECT ${errorText(e.reason)}`));
  await log(`START ${navigator.userAgent} ${window.innerWidth}x${window.innerHeight}`);
  try {
    await until(() => !!document.querySelector('.sidebar'), 10000);
    for (const view of ['games', 'tactics', 'engines', 'ai', 'settings', 'analyze'] as View[]) {
      useStore.getState().setView(view);
      await wait(500);
      await check(text(document.querySelector('main')).length > 20, `${view} view renders`);
    }
    await check(!!document.querySelector('.center-col .ai-panel'), 'conversation below the board');
    await check(!document.querySelector('.right-col .ai-panel'), 'right column reserved for evaluation, moves and puzzles');
    await check(!document.querySelector('.position-overview, .position-tabs, .agent-provider'), 'plain chat has no prompt tabs, provider picker or position details');
    if (cfg.user) {
      const before = (await api.games({})).total;
      useStore.getState().openImport('lichess');
      await until(() => !!document.querySelector('.modal input[placeholder="Any player’s username"]'), 5000);
      const input = document.querySelector('.modal input[placeholder="Any player’s username"]') as HTMLInputElement;
      setInput(input, cfg.user);
      await wait(250);
      button(/^Browse games$/, document.querySelector('.modal')!)?.click();
      await check(await until(() => !!document.querySelector('[aria-label="Game history"] .box'), 60000), 'remote history appears');
      await check((await api.games({})).total === before, 'browsing imports nothing');
      const rows = [...document.querySelectorAll('[aria-label="Game history"] .box')];
      const selected = rows.find((r) => r.querySelectorAll('b')[1]?.textContent?.toLowerCase() === cfg.user!.toLowerCase());
      await check(!!selected, 'history includes a game played as Black');
      await log(`SELECT ${text(selected!)}`);
      (selected!.querySelector('button') as HTMLButtonElement).click();
      await check(await until(() => useStore.getState().gameId !== null && !document.querySelector('.modal'), 15000), 'selected game imports and opens');
      const s = useStore.getState();
      await check((await api.games({})).total === before + 1, 'exactly one game imported');
      await check(s.orientation === 'black' && s.playerColor === 'black', 'Black account faces the player');
      await check(text(document.querySelector('.move-players')).includes('(you)'), 'move list identifies your side');
      await log(`GAME id=${s.gameId} white=${s.game?.white} black=${s.game?.black}`);
      button(/^Generate puzzles/)?.click();
      await check(await until(() => useStore.getState().scan !== null, 5000), 'Generate puzzles starts current-game scan');
      let previous = '';
      const end = Date.now() + 600000;
      while (useStore.getState().scan && Date.now() < end) {
        const progress = JSON.stringify(useStore.getState().scan?.progress);
        if (progress !== previous) { await log(`SCAN ${progress}`); previous = progress; }
        await wait(1500);
      }
      await check(useStore.getState().scan === null, 'scan completes');
      const tactics = await api.tacticsForGame(s.gameId!);
      await check(tactics.length > 0, 'current game produces tactics');
      await check(tactics.every((t) => t.gameId === s.gameId), 'all generated tactics belong to selected game');
      await log(`TACTICS ${tactics.length}`);
      for (const tactic of tactics) {
        let fen = tactic.fen;
        for (let i = 0; i < tactic.moves.length; i++) {
          const next = playUci(fen, tactic.moves[i]);
          await check(!!next, `tactic ${tactic.id} move ${i} legal`);
          fen = next!.fen;
          if (i % 2 === 1) {
            const verdict = await api.tacticCheck(tactic.id, i, tactic.moves[i]);
            await check(verdict.result === 'correct', `tactic ${tactic.id} solution move ${i} accepted`);
          }
        }
      }
      await check(await until(() => !!document.querySelector('.puzzle-status'), 5000), 'generated puzzle opens in trainer');
      await check(useStore.getState().view === 'analyze' && !!document.querySelector('.inline-trainer'), 'current-game puzzle stays in the analysis window');
      await check(!document.querySelector('.engine-panel'), 'engine answers hidden while solving');
      await until(() => /to play/.test(text(document.querySelector('.puzzle-status'))), 3000);
      await log(`TRAINER ${text(document.querySelector('.puzzle-status'))}`);
      const board = document.querySelector('.board')!.getBoundingClientRect();
      const square = document.querySelector('.board [data-square="a1"]')!.getBoundingClientRect();
      await check(Math.abs(board.width - board.height) < 1 && Math.abs(square.height * 8 - board.height) < 2 && Math.abs(square.width * 8 - board.width) < 2, 'native puzzle board and squares have matching geometry');

      button(/^Solution$/)?.click();
      await wait(1000);
      await check(text(document.querySelector('.puzzle-status')) === 'Solution shown', 'solution is accessible in trainer');
      button(/^Back to game$/)?.click();
      await check(await until(() => !!document.querySelector('.engine-panel'), 3000), 'Back to game restores analysis');
      await check(useStore.getState().tree.nodes[useStore.getState().current].fen === s.tree.nodes[s.current].fen, 'puzzle practice preserves the analyzed position');
      await api.importPgn('[White "Basic fixture"]\n[Black "Practice"]\n\n1. e4 e5 2. Bc4 Nc6 3. Qh5 Nf6 4. Qxf7# 1-0');
      await api.importPgn('[White "Quiet fixture"]\n[Black "Practice"]\n\n1. d4 d5 *');
      useStore.getState().setView('tactics');
      await check(await until(() => /White to play and mate/.test(text(document.querySelector('.puzzle-status'))), 5000), 'Puzzles opens with built-in basic practice');
      const batch = () => button(/^Generate puzzles from my games$/, document.querySelector('.batch-puzzles')!);
      await until(() => !!batch() && !batch()!.disabled, 3000);
      batch()?.click();
      await check(await until(() => useStore.getState().scan !== null, 3000), 'batch generation starts');
      await check(await until(() => useStore.getState().scan === null, 120000), 'batch generation finishes');
      const stats = await api.tacticsStats();
      await check(stats.scanned === 3, 'batch processes only the two remaining games, including a zero-puzzle game');
      const beforePuzzles = await api.tacticsForGame(s.gameId!);
      await until(() => !!batch() && !batch()!.disabled, 3000);
      batch()?.click();
      await until(() => useStore.getState().scan === null, 5000);
      await check((await api.tacticsStats()).scanned === stats.scanned && JSON.stringify(await api.tacticsForGame(s.gameId!)) === JSON.stringify(beforePuzzles), 'repeat batch skips all previously processed games');

    }
    const agent = await api.aiProviders();
    if (cfg.user && agent.settings.active) {
      useStore.getState().setView('analyze');
      await until(() => !!document.querySelector('.ai-input textarea:not(:disabled)'), 5000);
      const ask = async (question: string) => {
        const input = document.querySelector('.ai-input textarea') as HTMLTextAreaElement;
        Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, 'value')!.set!.call(input, question);
        input.dispatchEvent(new Event('input', { bubbles: true }));
        await wait(200);
        (document.querySelector('.ai-input button[type="submit"]') as HTMLButtonElement).click();
        await check(await until(() => useGameChat.getState().pending !== null, 5000), 'agent request starts');
        await check(await until(() => useGameChat.getState().pending === null, 120000), 'agent request finishes');
        const messages = useGameChat.getState().conversations[useStore.getState().analysisSession].messages;
        const answer = messages[messages.length - 1];
        await log(`AGENT ${answer.content}`);
        await check(answer.role === 'assistant' && !answer.error, 'live agent answers');
        return answer.content;
      };
      await ask('Remember the word bishop for my next question. Which color am I playing in this imported game? Reply in one sentence.');
      useStore.getState().next();
      await wait(300);
      const reply = await ask('Which color am I playing, and what word did I ask you to remember? Reply in one sentence.');
      await check(/black/i.test(reply) && /bishop/i.test(reply), 'agent retains conversation and player identity after moving');
    }
    if (cfg.canClear) {
      const settings = (await api.aiProviders()).settings;
      useStore.getState().setView('games'); await wait(600);
      button(/^Clear history$/)?.click();
      await check(await until(() => !!document.querySelector('.modal'), 2000), 'history clearing offers an explicit confirmation');
      button(/^Cancel$/, document.querySelector('.modal')!)?.click();
      await check((await api.games({})).total > 0, 'cancelling history clearing preserves games');
      button(/^Clear history$/)?.click(); await wait(200);
      button(/^Clear game history$/, document.querySelector('.modal')!)?.click();
      await check(await until(() => !document.querySelector('.modal'), 5000), 'history clear completes');
      await check((await api.games({})).total === 0 && (await api.tacticsStats()).tactics === 0, 'history clear removes games and generated puzzles');
      await check(JSON.stringify((await api.aiProviders()).settings) === JSON.stringify(settings), 'history clear preserves agent settings');
    }
    await log('RESULT PASS');
  } catch (e) { await log(`RESULT FAIL ${errorText(e)}`); }
  await invoke('selftest_done');
}
