// The typing test itself, free of any DOM so it can be unit-tested.
//
// A test is a list of target words and, in parallel, what was typed for each one. The caret
// lives in `inputs[index]`. Every keystroke appends a [ms, caret position] pair to `trace`, which
// is what a ghost replays later: the position is a character offset into the text (spaces
// included), so it maps onto any other word list.

export type Trace = [ms: number, pos: number][];

export interface CharCounts {
  correct: number;
  incorrect: number;
  extra: number;
  missed: number;
}

export interface Stats {
  wpm: number;
  raw: number;
  acc: number;
  chars: CharCounts;
  ms: number;
}

/** Typed characters past the end of a word are capped so a stuck key can't run away. */
export const MAX_EXTRA = 10;

export class TypingTest {
  readonly words: string[];
  inputs: string[] = [""];
  index = 0;
  keystrokes = 0;
  correctKeystrokes = 0;
  startedAt: number | null = null;
  endedAt: number | null = null;
  trace: Trace = [];

  constructor(words: string[]) {
    this.words = words;
  }

  get started(): boolean {
    return this.startedAt !== null;
  }

  get finished(): boolean {
    return this.endedAt !== null;
  }

  get input(): string {
    return this.inputs[this.index];
  }

  get word(): string {
    return this.words[this.index];
  }

  /** Milliseconds since the first keystroke (0 before it). */
  elapsed(now: number): number {
    if (this.startedAt === null) return 0;
    return (this.endedAt ?? now) - this.startedAt;
  }

  /** Character offset of the caret into the text, spaces included. */
  get pos(): number {
    let p = 0;
    for (let i = 0; i < this.index; i++) p += this.words[i].length + 1;
    return p + Math.min(this.input.length, this.word.length);
  }

  /** Lets time-mode tests grow as the typist (or the ghost) nears the end. */
  extend(more: string[]): void {
    this.words.push(...more);
  }

  private begin(now: number): void {
    if (this.startedAt === null) this.startedAt = now;
  }

  private record(now: number): void {
    this.trace.push([Math.round(this.elapsed(now)), this.pos]);
  }

  type(ch: string, now: number): void {
    if (this.finished) return;
    if (ch === " ") return this.space(now);
    if (this.input.length >= this.word.length + MAX_EXTRA) return;
    this.begin(now);
    this.keystrokes++;
    if (this.word[this.input.length] === ch) this.correctKeystrokes++;
    this.inputs[this.index] += ch;
    this.record(now);
  }

  space(now: number): void {
    if (this.finished || this.input.length === 0) return;
    this.begin(now);
    this.keystrokes++;
    if (this.input === this.word) this.correctKeystrokes++;
    if (this.index === this.words.length - 1) {
      this.record(now);
      this.endedAt = now;
      return;
    }
    this.index++;
    this.inputs.push("");
    this.record(now);
  }

  /** Backspace deletes a character; with `wholeWord` (⌥/⌃) it clears the word. On an empty word
   *  it steps back into the previous one, but only if that word was typed wrong. */
  backspace(now: number, wholeWord = false): void {
    if (this.finished || !this.started) return;
    if (this.input.length === 0) {
      if (this.index === 0) return;
      const prev = this.index - 1;
      if (this.inputs[prev] === this.words[prev]) return;
      this.inputs.pop();
      this.index = prev;
      if (wholeWord) this.inputs[prev] = "";
    } else {
      this.inputs[this.index] = wholeWord ? "" : this.input.slice(0, -1);
    }
    this.record(now);
  }

  /** True once the last word is typed exactly; words-mode tests end there without a space. */
  get lastWordDone(): boolean {
    return this.index === this.words.length - 1 && this.input === this.word;
  }

  finish(now: number): void {
    if (this.endedAt === null) this.endedAt = now;
  }

  counts(): CharCounts & { wordChars: number; typed: number } {
    const c = { correct: 0, incorrect: 0, extra: 0, missed: 0, wordChars: 0, typed: 0 };
    for (let i = 0; i <= this.index; i++) {
      const word = this.words[i];
      const input = this.inputs[i];
      const committed = i < this.index;
      for (let k = 0; k < input.length; k++) {
        if (k >= word.length) c.extra++;
        else if (input[k] === word[k]) c.correct++;
        else c.incorrect++;
      }
      c.typed += input.length + (committed ? 1 : 0);
      if (committed) {
        if (input.length < word.length) c.missed += word.length - input.length;
        // A correct word earns its letters plus the space after it.
        if (input === word) c.wordChars += word.length + 1;
      } else if (word.startsWith(input)) {
        // The word in progress counts as far as it's right, so a timed test that ends
        // mid-word doesn't throw those letters away.
        c.wordChars += input.length;
      }
    }
    return c;
  }

  stats(now: number): Stats {
    const ms = this.elapsed(now);
    const c = this.counts();
    const minutes = ms / 60000;
    const perMinute = (chars: number) => (minutes > 0 ? chars / 5 / minutes : 0);
    return {
      wpm: perMinute(c.wordChars),
      raw: perMinute(c.typed),
      acc: this.keystrokes > 0 ? (this.correctKeystrokes / this.keystrokes) * 100 : 100,
      chars: { correct: c.correct, incorrect: c.incorrect, extra: c.extra, missed: c.missed },
      ms,
    };
  }
}
