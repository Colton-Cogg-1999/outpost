import { useEffect, type ReactNode } from 'react';
import type { Classification, Score } from '../lib/types';
import { runs } from '../board/pieces';
import { useStore } from '../store';

// ───────────── icons (original line icons, 24-unit grid) ─────────────

const ICONS: Record<string, string> = {
  analyze: 'M8 5l11 7-11 7z',
  games: 'M4 6h2M4 12h2M4 18h2M9 6h11M9 12h11M9 18h11',
  puzzles: 'M12 2.5v4M12 17.5v4M2.5 12h4M17.5 12h4M12 7a5 5 0 1 0 0 10 5 5 0 0 0 0-10zM12 11v2',
  engines: 'M7 7h10v10H7zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4M10 10h4v4h-4z',
  ai: 'M12 3l2 6.5L20.5 12 14 14l-2 7-2-7-6.5-2L10 9.5z',
  settings: 'M4 6h9M17 6h3M4 12h3M11 12h9M4 18h11M19 18h1M13 4v4M7 10v4M15 16v4',
  antenna: 'M12 11v10M8 21h8M6.3 6.3a8 8 0 0 1 11.4 0M8.8 8.8a4.5 4.5 0 0 1 6.4 0M12 11.2v-.4',
  pawn: 'M12 3.5a2.6 2.6 0 1 1 0 5.2 2.6 2.6 0 0 1 0-5.2zM9.5 10.5h5M10 10.5l-1 7h6l-1-7M6.5 20.5h11v-3h-11z',
  file: 'M6 3h8l4 4v14H6zM14 3v4h4M9 12h6M9 16h6',
  image: 'M3 5h18v14H3zM3 16l5-5 4 4 3-3 6 6M16 8.5h.01',
  grid: 'M4 4h16v16H4zM4 12h16M12 4v16',
  flip: 'M7 20V4M4 7l3-3 3 3M17 4v16M14 17l3 3 3-3',
  first: 'M6 5v14M18 6l-7 6 7 6',
  prev: 'M15 6l-6 6 6 6',
  next: 'M9 6l6 6-6 6',
  last: 'M18 5v14M6 6l7 6-7 6',
  send: 'M4 12L20 4l-5 16-3-6.5z',
  close: 'M6 6l12 12M18 6L6 18',
  min: 'M6 13h12',
  max: 'M6 6h12v12H6z',
  chevron: 'M6 9l6 6 6-6',
  power: 'M12 3v8M7.1 6.5a7 7 0 1 0 9.8 0',
  plus: 'M12 5v14M5 12h14',
  minus: 'M5 12h14',
  trash: 'M5 7h14M10 7V4h4v3M7 7l1 13h8l1-13',
  star: 'M12 4l2.4 5 5.6.6-4.2 3.8 1.2 5.6L12 16.2 7 19l1.2-5.6L4 9.6 9.6 9z',
  save: 'M5 4h11l3 3v13H5zM8 4v5h7V4M8 20v-6h8v6',
  copy: 'M8 8h11v12H8zM5 16V4h11',
  paste: 'M8 4h8v3H8zM6 6H5v14h14V6h-1M9 12h6M9 16h4',
  refresh: 'M19 11a7 7 0 1 0-2 6M19 5v6h-6',
  stop: 'M7 7h10v10H7z',
  arrow: 'M5 19L18 6M10 6h8v8',
  review: 'M4 20V10M10 20V4M16 20v-7M22 20H2',
  spark: 'M12 3l2 6.5L20.5 12 14 14l-2 7-2-7-6.5-2L10 9.5z',
  hint: 'M9 18h6M10 21h4M12 3a6 6 0 0 0-3.5 10.9V16h7v-2.1A6 6 0 0 0 12 3z',
  eye: 'M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12zM12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z',
  folder: 'M3 6h6l2 2h10v11H3z',
  link: 'M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1',
  check: 'M5 12l4 4 10-10',
};

export function Icon({ name, className }: { name: keyof typeof ICONS | string; className?: string }) {
  return (
    <svg className={`icon ${className ?? ''}`} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth={1.7} strokeLinecap="square" strokeLinejoin="miter" aria-hidden="true">
      <path d={ICONS[name] ?? ''} />
    </svg>
  );
}

// ───────────── classification glyphs (original 9×9 pixel art) ─────────────

const GLYPHS: Record<Classification, string[]> = {
  brilliant: ['....#....', '....#....', '...###...', '.#######.', '#########', '.#######.', '...###...', '....#....', '....#....'],
  great: ['....#....', '...###...', '..##.##..', '.##...##.', '....#....', '...###...', '..##.##..', '.##...##.', '.........'],
  best: ['.........', '........#', '.......##', '......##.', '#....##..', '##..##...', '.####....', '..##.....', '.........'],
  good: ['.........', '...###...', '..#####..', '.#######.', '.#######.', '.#######.', '..#####..', '...###...', '.........'],
  inaccuracy: ['..####...', '.##..##..', '.....##..', '....##...', '...##....', '...##....', '.........', '...##....', '...##....'],
  mistake: ['.........', '.##...##.', '..##.##..', '...###...', '....#....', '...###...', '..##.##..', '.##...##.', '.........'],
  blunder: ['##.....##', '###...###', '.###.###.', '..#####..', '...###...', '..#####..', '.###.###.', '###...###', '##.....##'],
  forced: ['.........', '.#######.', '.#.....#.', '.#.....#.', '.#.....#.', '.#.....#.', '.#.....#.', '.#######.', '.........'],
};

export const CLASS_LABEL: Record<Classification, string> = {
  brilliant: 'Brilliant',
  great: 'Great',
  best: 'Best',
  good: 'Good',
  inaccuracy: 'Inaccuracy',
  mistake: 'Mistake',
  blunder: 'Blunder',
  forced: 'Only move',
};

/** Traditional annotation symbols, used for PGN export and screen readers. */
export const CLASS_NAG: Partial<Record<Classification, string>> = { brilliant: '!!', great: '!', inaccuracy: '?!', mistake: '?', blunder: '??' };

export function ClassIcon({ c }: { c: Classification }) {
  const rects = runs(GLYPHS[c], '#');
  return (
    <span className={`cls cls-${c}`} role="img" aria-label={CLASS_LABEL[c]} title={CLASS_LABEL[c]}>
      <svg viewBox="0 0 9 9" shapeRendering="crispEdges" fill="currentColor">
        {rects.map((r, i) => <rect key={i} x={r.x} y={r.y} width={r.w} height={1} />)}
      </svg>
    </span>
  );
}

// ───────────── scores ─────────────

export function formatScore(score: Score | null | undefined, terminal?: string | null): string {
  if (terminal === 'checkmate' && score) return score.value > 0 ? '1-0' : '0-1';
  if (terminal) return '½-½';
  if (!score) return '…';
  if (score.kind === 'mate') return score.value > 0 ? `#${score.value}` : `#-${Math.abs(score.value)}`;
  const v = score.value / 100;
  return `${v > 0 ? '+' : v < 0 ? '−' : ''}${Math.abs(v).toFixed(2)}`;
}

export function winPercent(score: Score | null | undefined): number {
  if (!score) return 50;
  if (score.kind === 'mate') return score.value > 0 ? 100 : score.value < 0 ? 0 : 50;
  const cp = Math.max(-1000, Math.min(1000, score.value));
  return 50 + 50 * (2 / (1 + Math.exp(-0.00368208 * cp)) - 1);
}

// ───────────── layout primitives ─────────────

export function Panel(props: { id?: string; title: ReactNode; right?: ReactNode; children: ReactNode; className?: string; collapsible?: boolean; bodyClass?: string }) {
  const collapsed = useStore((s) => (props.id ? !!s.prefs.collapsed[props.id] : false));
  const setPrefs = useStore((s) => s.setPrefs);
  const prefs = useStore((s) => s.prefs);
  const toggle = () => props.id && setPrefs({ collapsed: { ...prefs.collapsed, [props.id]: !collapsed } });
  return (
    <section className={`panel ${props.className ?? ''} ${collapsed ? 'collapsed' : ''}`}>
      <div className="panel-head">
        {props.collapsible !== false && props.id && (
          <button className="collapse" onClick={toggle} aria-expanded={!collapsed} aria-label={collapsed ? 'Expand panel' : 'Collapse panel'}>
            <Icon name="chevron" />
          </button>
        )}
        {typeof props.title === 'string' ? <h2>{props.title}</h2> : props.title}
        <div className="spacer" />
        {props.right}
      </div>
      {!collapsed && <div className={props.bodyClass ?? 'panel-body'}>{props.children}</div>}
    </section>
  );
}

export function Toggle({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label?: ReactNode }) {
  return (
    <label className={`toggle ${on ? 'on' : ''}`}>
      {label}
      <input type="checkbox" checked={on} onChange={(e) => onChange(e.target.checked)} style={{ position: 'absolute', opacity: 0, width: 0, height: 0 }} />
      <span className="track" aria-hidden="true" />
    </label>
  );
}

export function Modal({ title, onClose, children, footer, wide }: { title: ReactNode; onClose: () => void; children: ReactNode; footer?: ReactNode; wide?: boolean }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose();
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, [onClose]);
  return (
    <div className="modal-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="modal" role="dialog" aria-modal="true" style={wide ? { width: 'min(980px, calc(100% - 40px))' } : undefined}>
        <div className="panel-head">
          {typeof title === 'string' ? <h2>{title}</h2> : title}
          <div className="spacer" />
          <button className="btn ghost icon-only small" onClick={onClose} aria-label="Close">
            <Icon name="close" />
          </button>
        </div>
        <div className="modal-body">{children}</div>
        {footer && <div className="modal-foot">{footer}</div>}
      </div>
    </div>
  );
}

export function Progress({ value }: { value: number }) {
  return (
    <div className="progress" role="progressbar" aria-valuenow={Math.round(value * 100)} aria-valuemin={0} aria-valuemax={100}>
      <div style={{ width: `${Math.max(0, Math.min(1, value)) * 100}%` }} />
    </div>
  );
}

// ───────────── logo: an original pixel scene of a remote station ─────────────

export function Logo() {
  // 64×26 pixel canvas. Tower (a rook) with antenna, hall, radar dome, ridge, moon.
  const dome = [
    [40, 11, 5], [39, 12, 7], [38, 13, 9], [38, 14, 9], [38, 15, 9],
  ];
  return (
    <div className="logo">
      <svg viewBox="0 0 64 26" shapeRendering="crispEdges" aria-hidden="true">
        <g fill="currentColor" opacity="0.35">
          <path d="M0 22h5v-1h4v-1h3v-1h3v1h4v1h25v-1h4v-1h3v-1h3v1h3v1h7v4H0z" />
        </g>
        <g fill="currentColor">
          {/* moon */}
          <rect x="54" y="2" width="3" height="1" /><rect x="53" y="3" width="1" height="3" /><rect x="54" y="6" width="3" height="1" /><rect x="57" y="3" width="1" height="1" /><rect x="57" y="5" width="1" height="1" />
          {/* antenna + beacon */}
          <rect x="25" y="0" width="1" height="7" /><rect x="24" y="2" width="3" height="1" />
          {/* rook tower */}
          <rect x="21" y="7" width="2" height="2" /><rect x="24" y="7" width="3" height="2" /><rect x="28" y="7" width="2" height="2" />
          <rect x="21" y="9" width="9" height="1" /><rect x="22" y="10" width="7" height="8" />
          {/* hall */}
          <rect x="11" y="15" width="28" height="6" /><rect x="7" y="17" width="4" height="4" /><rect x="6" y="16" width="6" height="1" />
          {/* dome on pedestal */}
          {dome.map(([x, y, w], i) => <rect key={i} x={x} y={y} width={w} height={1} />)}
          <rect x="41" y="16" width="3" height="5" />
          {/* ground */}
          <rect x="4" y="21" width="44" height="1" />
        </g>
        <g fill="var(--on-phosphor)">
          <rect x="24" y="12" width="1" height="2" /><rect x="26" y="12" width="1" height="2" />
          <rect x="13" y="17" width="2" height="1" /><rect x="16" y="17" width="2" height="1" /><rect x="31" y="17" width="2" height="1" /><rect x="34" y="17" width="2" height="1" />
          <rect x="24" y="17" width="3" height="4" />
        </g>
        <rect className="beacon blink" x="25" y="0" width="1" height="1" fill="var(--amber)" />
      </svg>
      <div className="wordmark">OUTPOST</div>
      <div className="tagline">ANALYSIS · RESEARCH · UNDERSTANDING</div>
    </div>
  );
}
