import { mainline, type GameTree } from '../chess/tree';
import type { GameReport, MoveReview, Score } from '../lib/types';

/**
 * Attaches review results to tree nodes. Only nodes whose moves still match the
 * reviewed game get a classification: if the user edits the mainline, the review
 * stops applying at the first difference instead of mislabelling moves.
 */
export function reviewByNode(tree: GameTree, report: GameReport | null): Map<string, MoveReview> {
  const out = new Map<string, MoveReview>();
  if (!report || report.startFen !== tree.nodes[tree.root].fen) return out;
  const line = mainline(tree).slice(1);
  for (let i = 0; i < line.length && i < report.moves.length; i++) {
    const node = tree.nodes[line[i]];
    if (node.uci !== report.moves[i].uci) break;
    out.set(node.id, report.moves[i]);
  }
  return out;
}

export interface GraphPoint {
  id: string;
  ply: number;
  score: Score | null;
  cls?: MoveReview['classification'];
}

/** White-POV evaluation after every mainline position (root first). */
export function evalSeries(tree: GameTree, report: GameReport | null, nodeEvals: Record<string, { score: Score }>): GraphPoint[] {
  const ids = mainline(tree);
  const reviewed = reviewByNode(tree, report);
  return ids.map((id, i) => {
    const fromReport = report && i < report.positions.length && (i === 0 || reviewed.has(id)) ? report.positions[i].lines[0]?.score ?? null : null;
    return { id, ply: i, score: fromReport ?? nodeEvals[id]?.score ?? null, cls: reviewed.get(id)?.classification };
  });
}
