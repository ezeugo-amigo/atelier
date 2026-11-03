// Where the ghost of a personal best is at a given moment.
//
// "replay" walks the best run's own keystroke trace, so the ghost speeds up, hesitates, and even
// backspaces where you did. "steady" moves at that run's average speed. Both return a fractional
// character offset so the ghost caret can glide between letters instead of jumping.

import type { Trace } from "./engine";

export type GhostStyle = "replay" | "steady" | "off";

export interface GhostSource {
  wpm: number;
  trace: Trace;
}

/** Index of the last trace entry at or before `t`, or -1. */
function lastAtOrBefore(trace: Trace, t: number): number {
  let lo = 0;
  let hi = trace.length - 1;
  let found = -1;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (trace[mid][0] <= t) {
      found = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return found;
}

export function replayPos(trace: Trace, t: number): number {
  if (trace.length === 0 || t < 0) return 0;
  const i = lastAtOrBefore(trace, t);
  if (i === -1) return 0;
  const [t0, p0] = trace[i];
  const next = trace[i + 1];
  // Only glide toward a keystroke that moves forward by one letter; corrections and word
  // jumps snap, which reads as a real caret would.
  if (next && next[1] === p0 + 1 && next[0] > t0) {
    return p0 + (t - t0) / (next[0] - t0);
  }
  return p0;
}

export function steadyPos(wpm: number, t: number): number {
  return Math.max(0, (wpm * 5 * t) / 60000);
}

export function ghostPos(style: GhostStyle, source: GhostSource, t: number): number {
  if (style === "steady") return steadyPos(source.wpm, t);
  return replayPos(source.trace, t);
}
