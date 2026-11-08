# Move classification

OUTPOST separates two kinds of information, and the interface labels them differently:

- **Measured engine facts**: the engine's evaluation before and after each move, its preferred move, its principal variation, and the depth searched. These come straight from the UCI engine.
- **Heuristic labels**: Brilliant, Great, Best, Good, Inaccuracy, Mistake, Blunder, Only move. These are OUTPOST's interpretation of the measurements. They are not engine output and they are not Chess.com's or Lichess's labels.

The implementation is `src-tauri/src/analysis/classify.rs`. Every threshold below can be changed in **Settings → Game review**.

## The quantity that matters: expected-score loss

A centipawn is not worth the same everywhere. Losing 200 centipawns at +9.00 changes nothing; losing 200 centipawns at 0.00 can lose the game. So OUTPOST converts every evaluation into the mover's **winning chances** with the logistic curve Lichess publishes:

```
win% = 50 + 50 × (2 / (1 + exp(−0.00368208 × cp)) − 1)      cp clamped to ±1000
mate for the mover = 100%, mate against the mover = 0%
```

**Expected-score loss** is then

```
loss = win%(engine's best move, before the move) − win%(position after the move played)
```

measured in percentage points from the mover's side. When the move played is one of the engine's own MultiPV candidates, OUTPOST uses that candidate's score from the same search. If the move played is the engine's first choice, the loss is defined as 0, so search noise between two positions cannot penalise the best move.

## Labels

| Label | Rule (defaults) |
| --- | --- |
| Only move | It was the only legal move. |
| Best | Loss ≤ 1.0 point. |
| Good | Loss < 5 points. |
| Inaccuracy | 5 ≤ loss < 10 points. |
| Mistake | 10 ≤ loss < 15 points. |
| Blunder | Loss ≥ 15 points. |
| Great | The engine's first choice, **and** its second choice is at least 10 points worse, **and** the mover still has ≥ 20% afterwards, **and** the position was not already decided (mover < 95% before). This is the "only good move" case. |
| Brilliant | A **sound, non-trivial sacrifice**: see below. |

The inaccuracy, mistake and blunder thresholds (5, 10, 15) correspond to Lichess's published thresholds of 0.1, 0.2 and 0.3 on its [−1, 1] winning-chances scale.

The detail panel also names two concrete facts when they apply: **"Missed a forced mate in N"** and **"Allows a forced mate in N"**.

## Brilliant

Being the engine's top move is not enough. A move is Brilliant only if **all** of these hold:

1. **It is a real sacrifice.** After the move, the opponent can win at least 2 pawns of material (net of anything the move captured or promoted) by legally capturing on some square. The amount is measured by static exchange evaluation (SEE), the standard swap algorithm with x-ray attackers. A piece that was already loose before the move does not count as sacrificed by this move, unless it is the piece that moved.
2. **It is sound.** The expected-score loss is at most 2 points.
3. **It keeps a good game.** The mover still has at least 45% expected score.
4. **It is not trivial.** The mover was below 95% before the move, so sacrifices in positions that were already won do not count.

SEE ignores pins and checks, so the opponent's capture must also be legal in the actual position. The justification shown in the review ("offers about N pawns of material by static exchange, yet keeps X% expected score") is computed from these same numbers.

## Summary statistics

- **Average centipawn loss**: the mean over a side's moves of `max(0, best − played)` in centipawns, with evaluations clamped to ±1000 and mates counted as ±1000.
- **Mean move accuracy**: the mean of Lichess's per-move accuracy curve, `103.1668 × exp(−0.04354 × loss) − 3.1669`, clamped to 0–100. Lichess's own game accuracy uses a weighted and harmonic mean, so the numbers are not identical to Lichess's.
- **Turning points**: the up to five Mistakes or Blunders with the largest swing in White's winning chances, in game order.

## Known limitations

- Labels depend on depth. The default review depth is 16 per position; deeper reviews are more reliable and slower.
- SEE is static. A sacrifice whose point is a quiet follow-up is still recognised, because soundness comes from the engine, but a sacrifice of material that is "lost" only through a longer tactic is not.
- The Brilliant and Great rules are stricter than some sites'. That is deliberate.
- Different engines use different evaluation scales (Leela Chess Zero's centipawns are derived from its own win/draw/loss estimate). Review one game with one engine at a time.
