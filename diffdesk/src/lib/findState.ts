/**
 * Find-bar state machine.
 *
 * The bar is either closed or open, and `query` / `matchIndex` only exist while
 * it is open. That makes "a stale query on a closed bar" and "an index pointing
 * past the end of the results" unrepresentable, and it lets ⌘F while already
 * searching mean something distinct (refocus the input) from ⌘F while closed.
 *
 * `matchIndex` is clamped on every transition against the caller-supplied
 * `matchCount`, so the reducer never stores an index it could not resolve.
 */

export type FindState =
  | { kind: "closed" }
  | { kind: "open"; query: string; matchIndex: number; focusToken: number };

export type FindAction =
  | { type: "open" }
  | { type: "close" }
  | { type: "setQuery"; query: string }
  | { type: "step"; direction: 1 | -1; matchCount: number }
  | { type: "clampIndex"; matchCount: number };

export const initialFindState: FindState = { kind: "closed" };

export function findReducer(state: FindState, action: FindAction): FindState {
  switch (action.type) {
    case "open":
      // Already open means ⌘F was pressed during a search: keep the query and
      // position, bump the token so the input refocuses and selects.
      return state.kind === "open"
        ? { ...state, focusToken: state.focusToken + 1 }
        : { kind: "open", query: "", matchIndex: 0, focusToken: 0 };

    case "close":
      return { kind: "closed" };

    case "setQuery":
      if (state.kind !== "open") return state;
      if (state.query === action.query) return state;
      return { ...state, query: action.query, matchIndex: 0 };

    case "step": {
      if (state.kind !== "open" || action.matchCount === 0) return state;
      const next = state.matchIndex + action.direction;
      const wrapped =
        next < 0
          ? action.matchCount - 1
          : next >= action.matchCount
            ? 0
            : next;
      return { ...state, matchIndex: wrapped };
    }

    case "clampIndex": {
      if (state.kind !== "open") return state;
      const clamped = clampIndex(state.matchIndex, action.matchCount);
      return clamped === state.matchIndex ? state : { ...state, matchIndex: clamped };
    }
  }
}

export function clampIndex(index: number, matchCount: number): number {
  if (matchCount === 0) return 0;
  return Math.min(Math.max(index, 0), matchCount - 1);
}

export function findQuery(state: FindState): string {
  return state.kind === "open" ? state.query : "";
}
