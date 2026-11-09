import { useMemo } from 'react';
import { winPercent } from '../components/ui';
import type { GraphPoint } from './analysis';

const W = 380;
const H = 52;
const PAD_L = 4;
const PAD_R = 4;
const PAD_Y = 6;
const LABELS: [string, number][] = [['+3', 300], ['+1', 100], ['0', 0], ['−1', -100], ['−3', -300]];

/** Win-percentage scale: ±1 and ±3 pawns sit at sensible heights and mates stay on-chart. */
export function EvalGraph({ points, current, onSelect }: { points: GraphPoint[]; current: string; onSelect: (id: string) => void }) {
  const n = Math.max(points.length - 1, 1);
  const x = (i: number) => PAD_L + (i / n) * (W - PAD_L - PAD_R);
  const y = (pct: number) => PAD_Y + (1 - pct / 100) * (H - 2 * PAD_Y);
  const known = useMemo(() => points.map((p, i) => ({ ...p, i })).filter((p) => p.score), [points]);
  const path = known.map((p, k) => `${k ? 'L' : 'M'}${x(p.i).toFixed(1)},${y(winPercent(p.score)).toFixed(1)}`).join('');
  const area = known.length > 1 ? `${path}L${x(known[known.length - 1].i)},${H - PAD_Y}L${x(known[0].i)},${H - PAD_Y}Z` : '';
  const currentIndex = points.findIndex((p) => p.id === current);

  const pick = (e: React.MouseEvent<SVGSVGElement>) => {
    const rect = e.currentTarget.getBoundingClientRect();
    const px = ((e.clientX - rect.left) / rect.width) * W;
    const i = Math.round(((px - PAD_L) / (W - PAD_L - PAD_R)) * n);
    const p = points[Math.max(0, Math.min(points.length - 1, i))];
    if (p) onSelect(p.id);
  };

  return (
    <svg className="graph" viewBox={`0 0 ${W} ${H}`} preserveAspectRatio="none" onClick={pick} role="img" aria-label="Evaluation graph. Click to jump to a move.">
      <g className="axis">
        {LABELS.map(([label, cp]) => {
          const yy = y(winPercent({ kind: 'cp', value: cp }));
          return (
            <g key={label}>
              <line className={cp === 0 ? 'zero' : 'grid'} x1={PAD_L} x2={W - PAD_R} y1={yy} y2={yy} strokeDasharray={cp === 0 ? undefined : '1 4'} />
              <text x={PAD_L - 4} y={yy + 3} textAnchor="end">{label}</text>
            </g>
          );
        })}
      </g>
      {area && <path className="area" d={area} />}
      {path && <path className="curve" d={path} vectorEffect="non-scaling-stroke" />}
      {known.map((p) => (
        <rect
          key={p.id}
          className={`dot ${p.cls ?? ''}`}
          x={x(p.i) - (p.cls === 'blunder' || p.cls === 'mistake' ? 2 : 1.2)}
          y={y(winPercent(p.score)) - (p.cls === 'blunder' || p.cls === 'mistake' ? 2 : 1.2)}
          width={p.cls === 'blunder' || p.cls === 'mistake' ? 4 : 2.4}
          height={p.cls === 'blunder' || p.cls === 'mistake' ? 4 : 2.4}
        />
      ))}
      {known.find((p) => p.i === currentIndex) && <circle className="current-dot" cx={x(currentIndex)} cy={y(winPercent(known.find((p) => p.i === currentIndex)!.score))} r={3.5} fill="var(--phosphor)" />}
      {currentIndex >= 0 && <line className="cursor" x1={x(currentIndex)} x2={x(currentIndex)} y1={PAD_Y - 4} y2={H - PAD_Y + 4} />}
    </svg>
  );
}
