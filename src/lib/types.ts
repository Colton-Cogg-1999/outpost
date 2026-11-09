// Mirrors of the Rust types that cross the IPC boundary.

export type Score = { kind: 'cp'; value: number } | { kind: 'mate'; value: number };

export interface PvLine {
  multipv: number;
  depth: number;
  score: Score; // White's point of view
  rawScore: Score; // as reported (side to move)
  wdl: [number, number, number] | null;
  san: string[];
  uci: string[];
}

export interface Snapshot {
  searchId: number;
  engine: string;
  fen: string;
  depth: number;
  seldepth: number | null;
  nodes: number | null;
  nps: number | null;
  timeMs: number | null;
  hashfull: number | null;
  lines: PvLine[];
  terminal: string | null;
  cached: boolean;
}

export type SearchEvent =
  | { type: 'update'; snapshot: Snapshot }
  | { type: 'done'; snapshot: Snapshot; bestMove: string | null }
  | { type: 'error'; searchId: number; message: string };

export interface SearchLimit {
  depth?: number | null;
  movetimeMs?: number | null;
  nodes?: number | null;
}

export interface UciOption {
  name: string;
  kind: 'check' | 'spin' | 'combo' | 'button' | 'string';
  default: string | null;
  min: number | null;
  max: number | null;
  vars: string[];
}

export interface EngineRecord {
  id: string;
  name: string;
  path: string;
  author: string | null;
  options: Record<string, string>;
  availableOptions: UciOption[];
  managed: boolean;
  builtin: boolean;
  createdAt: number;
  running: boolean;
  default: boolean;
  exists: boolean;
}

export type Classification = 'brilliant' | 'great' | 'best' | 'good' | 'inaccuracy' | 'mistake' | 'blunder' | 'forced';

export interface MoveReview {
  ply: number;
  color: 'white' | 'black';
  san: string;
  uci: string;
  bestSan: string | null;
  bestUci: string | null;
  bestLine: string[];
  evalBefore: Score;
  evalAfter: Score;
  classification: Classification;
  winBefore: number;
  winAfter: number;
  winLoss: number;
  cpLoss: number;
  accuracy: number;
  reasons: string[];
  sacrifice: number | null;
}

export interface SideSummary {
  acpl: number;
  accuracy: number;
  moves: number;
  counts: Partial<Record<Classification, number>>;
}

export interface PositionEval {
  fen: string;
  depth: number;
  lines: PvLine[];
  terminal: string | null;
  cached: boolean;
}

export interface GameReport {
  engine: string;
  settings: ReviewSettings;
  startFen: string;
  positions: PositionEval[];
  moves: MoveReview[];
  white: SideSummary;
  black: SideSummary;
  turningPoints: { ply: number; swing: number }[];
  createdAt: number;
}

export interface ClassifyConfig {
  bestTolerance: number;
  inaccuracy: number;
  mistake: number;
  blunder: number;
  greatGap: number;
  decidedAbove: number;
  sacrificeMin: number;
  brilliantMaxLoss: number;
  brilliantMinAfter: number;
}

export interface ReviewSettings {
  depth: number | null;
  movetimeMs: number | null;
  multipv: number;
  classify: ClassifyConfig;
}

export type ReviewEvent =
  | { type: 'progress'; done: number; total: number; cached: number; ply: number; score: Score }
  | { type: 'done'; report: GameReport }
  | { type: 'error'; message: string }
  | { type: 'cancelled' };

export interface GameSummary {
  id: number;
  source: string;
  account: string | null;
  url: string | null;
  white: string;
  black: string;
  whiteElo: number | null;
  blackElo: number | null;
  result: string;
  date: string | null;
  playedAt: number | null;
  timeControl: string | null;
  speed: string | null;
  event: string | null;
  eco: string | null;
  opening: string | null;
  plyCount: number;
  favorite: boolean;
  analyzed: boolean;
  importedAt: number;
}

export interface GameDetail extends GameSummary {
  pgn: string;
}

export interface ImportSummary {
  imported: number;
  duplicates: number;
  failed: number;
  errors: string[];
  firstId: number | null;
}

export interface ImportProgress {
  message: string;
  scanned: number;
  imported: number;
  duplicates: number;
  skipped: number;
  done: boolean;
}

export interface Tactic {
  id: number;
  gameId: number;
  ply: number;
  /** Position before the opponent's mistake. */
  fen: string;
  /** The mistake, then the solution. The solver plays moves 1, 3, 5, … */
  moves: string[];
  solver: 'white' | 'black';
  kind: 'mate' | 'advantage';
  cp: number;
  themes: string[];
  owner: 'you' | 'opponent' | null;
  played: string | null;
  attempts: number;
  solved: number;
  streak: number;
  dueAt: number;
  white: string;
  black: string;
  date: string | null;
  url: string | null;
}

export type TacticVerdict =
  | { result: 'correct'; reply: string | null; solved: boolean; alternativeMate: boolean }
  | { result: 'incorrect'; expected: string };

export interface TacticStats {
  games: number;
  scanned: number;
  tactics: number;
  yours: number;
  missed: number;
  due: number;
  fresh: number;
  attempts: number;
  solvedFirstTry: number;
  streak: number;
}

export interface ScanProgress {
  gameIndex: number;
  games: number;
  gameId: number;
  label: string;
  stage: '' | 'review' | 'search';
  done: number;
  total: number;
  found: number;
  finished: boolean;
  cancelled?: boolean;
  failed?: number;
}

export type Strictness = 'personal' | 'lichess';

export interface ProviderInfo {
  id: string;
  name: string;
  kind: 'cli' | 'api' | 'local';
  needsKey: boolean;
  defaultBaseUrl: string | null;
  description: string;
}

export interface ProviderConfig {
  model?: string | null;
  baseUrl?: string | null;
}

export interface AiSettings {
  active: string | null;
  providers: Record<string, ProviderConfig>;
  skillLevel: string;
  saveChats: boolean;
  length: 'short' | 'normal' | 'detailed';
}

export interface ProviderStatus {
  id: string;
  ready: boolean;
  detail: string;
  plan: string | null;
  models: { id: string; label: string }[];
}

export interface KeyStatus {
  source: 'keyring' | 'environment' | 'none';
  storeAvailable: boolean;
}

export interface ValidatedMove {
  san: string;
  uci: string;
}

export interface MoveCheck {
  keyMoves: ValidatedMove[];
  mentioned: ValidatedMove[];
  rejected: string[];
}

export interface AiAnswer {
  text: string;
  moves: MoveCheck;
  provider: string;
  model: string | null;
  chatId: string | null;
  cancelled: boolean;
}

export interface EngineDefaults {
  threads: number;
  hashMb: number;
}

export interface AppInfo {
  version: string;
  dataDir: string;
  os: string;
  arch: string;
  cores: number;
  engineDefaults: EngineDefaults;
}

/** A history row is transient; its PGN is saved only when selected. */
export interface HistoryGame {
  account?: string | null;
  source: string;
  sourceId: string | null;
  white: string;
  black: string;
  whiteElo: number | null;
  blackElo: number | null;
  result: string;
  date: string | null;
  speed: string | null;
  timeControl: string | null;
  pgn: string;
}
export interface HistoryPage { games: HistoryGame[]; nextCursor: number | null }
