import { describe, expect, test } from "bun:test";
import { TypingTest } from "../src/engine";
import { replayPos, steadyPos } from "../src/ghost";
import { newProfile, recordResult, type Best } from "../src/store";

/** Types `text` one key every `gap` ms starting at t=0; "<" is backspace. */
function typeAll(t: TypingTest, text: string, gap = 100): number {
  let now = 0;
  for (const ch of text) {
    if (ch === "<") t.backspace(now);
    else t.type(ch, now);
    now += gap;
  }
  return now - gap;
}

describe("TypingTest", () => {
  test("perfect typing counts every letter and space", () => {
    const t = new TypingTest(["hello", "world"]);
    const end = typeAll(t, "hello world", 100);
    expect(t.lastWordDone).toBe(true);
    const s = t.stats(end);
    // 11 chars in 1s -> 11/5 words per 1/60 minute.
    expect(s.wpm).toBeCloseTo((11 / 5) * 60, 5);
    expect(s.acc).toBe(100);
    expect(s.chars).toEqual({ correct: 10, incorrect: 0, extra: 0, missed: 0 });
  });

  test("a wrong word earns no word chars but still counts as raw", () => {
    const t = new TypingTest(["cat", "dog"]);
    const end = typeAll(t, "cut dog");
    const s = t.stats(end);
    expect(s.chars.incorrect).toBe(1);
    expect(s.wpm).toBeCloseTo((3 / 5) * (60000 / end), 5);
    expect(s.raw).toBeCloseTo((7 / 5) * (60000 / end), 5);
    expect(s.acc).toBeCloseTo((5 / 7) * 100, 5);
  });

  test("backspace steps back only into a wrong word", () => {
    const t = new TypingTest(["ab", "cd", "ef"]);
    typeAll(t, "ab ");
    t.backspace(1000);
    expect(t.index).toBe(1); // "ab" was right, so it's locked
    typeAll(t, "xd ");
    expect(t.index).toBe(2);
    t.backspace(2000);
    expect(t.index).toBe(1);
    expect(t.input).toBe("xd");
  });

  test("extra letters are capped and tracked", () => {
    const t = new TypingTest(["a", "b"]);
    typeAll(t, "a" + "z".repeat(20));
    expect(t.input.length).toBe(11);
    expect(t.counts().extra).toBe(10);
  });

  test("trace records the caret position after every key", () => {
    const t = new TypingTest(["hi", "yo"]);
    typeAll(t, "hi y<y");
    expect(t.trace).toEqual([
      [0, 1],
      [100, 2],
      [200, 3],
      [300, 4],
      [400, 3],
      [500, 4],
    ]);
  });

  test("space on the last word ends the test", () => {
    const t = new TypingTest(["one"]);
    typeAll(t, "onx ");
    expect(t.finished).toBe(true);
  });
});

describe("ghost", () => {
  const trace: [number, number][] = [
    [0, 1],
    [100, 2],
    [200, 3],
    [300, 2], // a correction
    [400, 3],
  ];

  test("replay glides between forward keystrokes", () => {
    expect(replayPos(trace, 0)).toBe(1);
    expect(replayPos(trace, 50)).toBeCloseTo(1.5);
    expect(replayPos(trace, 150)).toBeCloseTo(2.5);
  });

  test("replay snaps on corrections and holds after the end", () => {
    expect(replayPos(trace, 250)).toBe(3);
    expect(replayPos(trace, 300)).toBe(2);
    expect(replayPos(trace, 5000)).toBe(3);
  });

  test("steady moves at the best's average speed", () => {
    // 120 wpm = 600 chars/minute = 10 chars/second.
    expect(steadyPos(120, 1000)).toBeCloseTo(10);
  });
});

describe("personal bests", () => {
  const run = (wpm: number, acc = 95): Best => ({
    id: String(wpm),
    mode: "time-30",
    at: "",
    wpm,
    raw: wpm,
    acc,
    ms: 30000,
    chars: { correct: 0, incorrect: 0, extra: 0, missed: 0 },
    samples: [],
    trace: [[0, 1]],
  });

  test("only a faster run replaces the ghost", () => {
    const p = newProfile("Ada");
    expect(recordResult(p, run(80))).toBeNull();
    expect(recordResult(p, run(70))).toBeUndefined();
    expect(recordResult(p, run(90))?.wpm).toBe(80);
    expect(p.bests["time-30"].wpm).toBe(90);
    expect(p.results).toHaveLength(3);
    expect("trace" in p.results[0]).toBe(false);
  });
});
