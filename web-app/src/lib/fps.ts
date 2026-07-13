/**
 * Exponential moving average of instantaneous FPS from a frame delta in ms.
 * `prevEma <= 0` is treated as a cold start and snaps to the first reading.
 * Non-positive `dtMs` returns `prevEma` unchanged (ignores bad samples).
 */
export function emaFps(prevEma: number, dtMs: number): number {
  if (dtMs <= 0) return prevEma;
  const inst = 1000 / dtMs;
  if (prevEma <= 0) return inst;
  return prevEma * 0.9 + inst * 0.1;
}
