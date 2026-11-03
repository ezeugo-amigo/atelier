// Profiles, results, and personal bests, persisted as one JSON document.
//
// Inside the desktop app the document lives in the app's data directory (see src-tauri); in a
// plain browser (`bun run vite`) it falls back to localStorage so the UI can be worked on
// without the shell.

import type { CharCounts, Trace } from "./engine";
import type { GhostStyle } from "./ghost";

export type ModeKind = "time" | "words";

export interface Mode {
  kind: ModeKind;
  value: number;
}

export const MODE_VALUES: Record<ModeKind, number[]> = {
  time: [15, 30, 60, 120],
  words: [10, 25, 50, 100],
};

export const modeKey = (m: Mode): string => `${m.kind}-${m.value}`;
export const modeLabel = (m: Mode): string => (m.kind === "time" ? `${m.value}s` : `${m.value} words`);

export interface Result {
  id: string;
  mode: string;
  at: string;
  wpm: number;
  raw: number;
  acc: number;
  ms: number;
  chars: CharCounts;
  /** Cumulative wpm at the end of each second, for the results chart. */
  samples: number[];
}

/** A personal best keeps its keystroke trace so it can be replayed as a ghost. */
export interface Best extends Result {
  trace: Trace;
}

export type Theme = "paper" | "cocoa";

export interface Settings {
  mode: Mode;
  ghost: GhostStyle;
  theme: Theme;
}

export interface Profile {
  id: string;
  name: string;
  /** SHA-256 of salt + PIN; absent when the profile is open to anyone at the computer. */
  pin?: { salt: string; hash: string };
  createdAt: string;
  settings: Settings;
  results: Result[];
  bests: Record<string, Best>;
}

export interface Db {
  version: 1;
  profiles: Profile[];
}

/** Older results beyond this are dropped; personal bests are kept regardless. */
const MAX_RESULTS = 1000;

const STORAGE_KEY = "ghost.db";

const emptyDb = (): Db => ({ version: 1, profiles: [] });

type Invoke = (cmd: string, args?: Record<string, unknown>) => Promise<unknown>;

async function tauriInvoke(): Promise<Invoke | null> {
  if (!("__TAURI_INTERNALS__" in window)) return null;
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke;
}

export async function loadDb(): Promise<Db> {
  const invoke = await tauriInvoke();
  const text = invoke ? ((await invoke("load_db")) as string | null) : localStorage.getItem(STORAGE_KEY);
  if (!text) return emptyDb();
  try {
    const db = JSON.parse(text) as Db;
    return db.version === 1 && Array.isArray(db.profiles) ? db : emptyDb();
  } catch {
    return emptyDb();
  }
}

let pending: Promise<void> = Promise.resolve();

/** Saves are chained so two quick writes can't land out of order. */
export function saveDb(db: Db): Promise<void> {
  const text = JSON.stringify(db);
  pending = pending.then(async () => {
    const invoke = await tauriInvoke();
    if (invoke) await invoke("save_db", { data: text });
    else localStorage.setItem(STORAGE_KEY, text);
  });
  return pending;
}

export function uid(): string {
  return crypto.randomUUID();
}

async function sha256(text: string): Promise<string> {
  const bytes = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(text));
  return [...new Uint8Array(bytes)].map((b) => b.toString(16).padStart(2, "0")).join("");
}

export async function hashPin(pin: string): Promise<{ salt: string; hash: string }> {
  const salt = uid();
  return { salt, hash: await sha256(salt + pin) };
}

export async function checkPin(profile: Profile, pin: string): Promise<boolean> {
  if (!profile.pin) return true;
  return (await sha256(profile.pin.salt + pin)) === profile.pin.hash;
}

export function newProfile(name: string): Profile {
  return {
    id: uid(),
    name,
    createdAt: new Date().toISOString(),
    settings: { mode: { kind: "time", value: 30 }, ghost: "replay", theme: "paper" },
    results: [],
    bests: {},
  };
}

/** Records a finished test. Returns the best it replaced (or null if it set the first one) when
 *  this run is a new personal best, and undefined when it isn't. */
export function recordResult(profile: Profile, best: Best): Best | null | undefined {
  const { trace: _trace, ...result } = best;
  profile.results.push(result);
  if (profile.results.length > MAX_RESULTS) profile.results.splice(0, profile.results.length - MAX_RESULTS);
  const previous = profile.bests[best.mode];
  if (previous && !isBetter(best, previous)) return undefined;
  profile.bests[best.mode] = best;
  return previous ?? null;
}

function isBetter(a: Result, b: Result): boolean {
  if (a.wpm !== b.wpm) return a.wpm > b.wpm;
  return a.acc > b.acc;
}
