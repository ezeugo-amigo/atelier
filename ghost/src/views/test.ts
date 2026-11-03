// The typing screen: three lines of words, your caret, and the ghost of your best run.

import { h } from "../dom";
import { TypingTest } from "../engine";
import { ghostPos, type GhostSource, type GhostStyle } from "../ghost";
import { modeKey, type Best, type Mode } from "../store";
import { randomWords } from "../words";

export interface TestOptions {
  mode: Mode;
  ghost: GhostSource | null;
  ghostStyle: GhostStyle;
  onFinish: (run: Best) => void;
}

/** Time-mode tests start with this many words and grow as anyone gets close to the end. */
const INITIAL_TIME_WORDS = 80;
const GROW_BY = 40;
const VISIBLE_LINES = 3;

interface Point {
  x: number;
  y: number;
}

export class TestView {
  readonly el: HTMLElement;
  private test!: TypingTest;
  private wordsEl!: HTMLElement;
  private wordEls: HTMLElement[] = [];
  private caret = h("div", { class: "caret" });
  private ghostCaret = h("div", { class: "caret caret--ghost" });
  private timerEl = h("span", { class: "timer" });
  private deltaEl = h("span", { class: "delta" });
  private viewport = h("div", { class: "words-viewport" });
  private firstLine = 0;
  private samples: number[] = [];
  private frame = 0;

  constructor(private opts: TestOptions) {
    this.el = h(
      "main",
      { class: "test" },
      h("div", { class: "test-meta" }, this.timerEl, this.deltaEl),
      this.viewport,
      h("p", { class: "hint" }, h("kbd", null, "tab"), " or ", h("kbd", null, "⌘R"), " restart"),
    );
    this.reset();
  }

  private get ghostOn(): boolean {
    return this.opts.ghost !== null && this.opts.ghostStyle !== "off";
  }

  reset(): void {
    cancelAnimationFrame(this.frame);
    const { mode } = this.opts;
    this.test = new TypingTest(randomWords(mode.kind === "words" ? mode.value : INITIAL_TIME_WORDS));
    this.samples = [];
    this.firstLine = 0;
    this.wordsEl = h("div", { class: "words" });
    this.wordEls = this.test.words.map((_, i) => this.buildWord(i));
    this.wordsEl.append(...this.wordEls, this.ghostCaret, this.caret);
    this.ghostCaret.hidden = !this.ghostOn;
    this.viewport.replaceChildren(this.wordsEl);
    document.body.classList.remove("typing");
    this.renderMeta(0);
    // Layout needs the element in the document; place carets on the next frame.
    requestAnimationFrame(() => this.place());
  }

  destroy(): void {
    cancelAnimationFrame(this.frame);
    document.body.classList.remove("typing");
  }

  // ---- input ----

  onKey(e: KeyboardEvent): void {
    if (e.key === "Tab" || e.key === "Escape" || isCmdR(e)) {
      e.preventDefault();
      this.reset();
      return;
    }
    const test = this.test;
    if (test.finished) return;
    const now = performance.now();
    const before = test.index;
    if (e.key === "Backspace") {
      e.preventDefault();
      test.backspace(now, e.altKey || e.ctrlKey || e.metaKey);
    } else if (e.key.length === 1 && !e.metaKey && !e.ctrlKey) {
      e.preventDefault();
      const starting = !test.started;
      test.type(e.key, now);
      if (starting && test.started) this.start();
    } else {
      return;
    }
    this.renderWord(before);
    if (test.index !== before) this.renderWord(test.index);
    this.grow();
    this.place();
    if (test.finished || (this.opts.mode.kind === "words" && test.lastWordDone)) this.finish(now);
  }

  private start(): void {
    document.body.classList.add("typing");
    const tick = () => {
      const now = performance.now();
      const ms = this.test.elapsed(now);
      // Cumulative wpm at each whole second, for the results chart.
      while (this.samples.length < Math.floor(ms / 1000)) {
        const at = this.test.startedAt! + (this.samples.length + 1) * 1000;
        this.samples.push(this.test.stats(at).wpm);
      }
      if (this.opts.mode.kind === "time" && ms >= this.opts.mode.value * 1000) {
        this.finish(this.test.startedAt! + this.opts.mode.value * 1000);
        return;
      }
      this.grow();
      this.placeGhost(ms);
      this.renderMeta(ms);
      this.frame = requestAnimationFrame(tick);
    };
    this.frame = requestAnimationFrame(tick);
  }

  private finish(now: number): void {
    cancelAnimationFrame(this.frame);
    const test = this.test;
    test.finish(now);
    const stats = test.stats(now);
    const lastSecond = this.samples.length * 1000;
    if (stats.ms > lastSecond + 250 || this.samples.length === 0) this.samples.push(stats.wpm);
    document.body.classList.remove("typing");
    this.opts.onFinish({
      id: crypto.randomUUID(),
      mode: modeKey(this.opts.mode),
      at: new Date().toISOString(),
      wpm: round2(stats.wpm),
      raw: round2(stats.raw),
      acc: round2(stats.acc),
      ms: Math.round(stats.ms),
      chars: stats.chars,
      samples: this.samples.map(round2),
      trace: test.trace,
    });
  }

  // ---- rendering ----

  private buildWord(i: number): HTMLElement {
    const el = h("span", { class: "word" });
    this.fillWord(el, i);
    return el;
  }

  private fillWord(el: HTMLElement, i: number): void {
    const word = this.test.words[i];
    const input = this.test.inputs[i] ?? "";
    const letters: HTMLElement[] = [];
    for (let k = 0; k < Math.max(word.length, input.length); k++) {
      let cls = "ch";
      if (k >= word.length) cls += " ch--extra";
      else if (k < input.length) cls += input[k] === word[k] ? " ch--ok" : " ch--bad";
      letters.push(h("span", { class: cls }, k < word.length ? word[k] : input[k]));
    }
    el.replaceChildren(...letters);
    const committed = i < this.test.index;
    el.classList.toggle("word--wrong", committed && input !== word);
  }

  private renderWord(i: number): void {
    // Re-render neighbors too: stepping back or forward changes whether they count as committed.
    for (const k of [i - 1, i, i + 1]) {
      const el = this.wordEls[k];
      if (el) this.fillWord(el, k);
    }
  }

  /** Keeps at least GROW_BY words beyond whoever is furthest along in a timed test. */
  private grow(): void {
    if (this.opts.mode.kind !== "time") return;
    const test = this.test;
    const ghostWord = this.wordAt(this.ghostAt(test.elapsed(performance.now()))).word;
    const furthest = Math.max(test.index, ghostWord);
    if (test.words.length - furthest > GROW_BY) return;
    const start = test.words.length;
    test.extend(randomWords(GROW_BY));
    for (let i = start; i < test.words.length; i++) {
      const el = this.buildWord(i);
      this.wordEls.push(el);
      this.wordsEl.insertBefore(el, this.ghostCaret);
    }
  }

  /** The ghost's text position `ms` into the test; it waits at the start until you begin. */
  private ghostAt(ms: number): number {
    if (!this.ghostOn || !this.test.started) return 0;
    return ghostPos(this.opts.ghostStyle, this.opts.ghost!, ms);
  }

  /** Word index and letter offset for a character position in the text. */
  private wordAt(pos: number): { word: number; letter: number } {
    const words = this.test.words;
    let start = 0;
    for (let i = 0; i < words.length; i++) {
      if (pos <= start + words[i].length) return { word: i, letter: pos - start };
      start += words[i].length + 1;
    }
    const last = words.length - 1;
    return { word: last, letter: words[last].length };
  }

  /** Top-left of the caret slot before `letter` in word `i` (or after its last letter). */
  private slot(i: number, letter: number): Point {
    const wordEl = this.wordEls[i];
    const letters = wordEl.children as HTMLCollectionOf<HTMLElement>;
    if (letter < letters.length) {
      const ch = letters[letter];
      return { x: ch.offsetLeft, y: ch.offsetTop };
    }
    const last = letters[letters.length - 1];
    return { x: last.offsetLeft + last.offsetWidth, y: last.offsetTop };
  }

  /** Point for a fractional text position: glides along a line, snaps across a wrap. */
  private point(pos: number): Point {
    const base = Math.floor(pos);
    const a = this.wordAt(base);
    const p0 = this.slot(a.word, a.letter);
    const frac = pos - base;
    if (frac === 0) return p0;
    const b = this.wordAt(base + 1);
    const p1 = this.slot(b.word, b.letter);
    if (p1.y !== p0.y) return p0;
    return { x: p0.x + (p1.x - p0.x) * frac, y: p0.y };
  }

  private lineHeight(): number {
    return parseFloat(getComputedStyle(this.wordsEl).lineHeight);
  }

  private place(): void {
    const test = this.test;
    const caretAt = this.slot(test.index, test.input.length);
    const lh = this.lineHeight();
    const line = Math.round(caretAt.y / lh);
    // Your caret rides the middle line once you're past the first; backspacing up scrolls back.
    if (line > this.firstLine + 1) this.firstLine = line - 1;
    if (line < this.firstLine) this.firstLine = line;
    this.wordsEl.style.transform = `translateY(${-this.firstLine * lh}px)`;
    this.viewport.style.height = `${VISIBLE_LINES * lh}px`;
    this.caret.style.transform = `translate(${caretAt.x}px, ${caretAt.y}px)`;
    this.placeGhost(test.elapsed(performance.now()));
  }

  private placeGhost(ms: number): void {
    if (!this.ghostOn) return;
    const p = this.point(this.ghostAt(ms));
    this.ghostCaret.style.transform = `translate(${p.x}px, ${p.y}px)`;
  }

  private renderMeta(ms: number): void {
    const { mode } = this.opts;
    const test = this.test;
    if (mode.kind === "time") {
      this.timerEl.textContent = String(Math.max(0, Math.ceil(mode.value - ms / 1000)));
    } else {
      this.timerEl.textContent = `${Math.min(test.index + (test.lastWordDone ? 1 : 0), mode.value)}/${mode.value}`;
    }
    this.deltaEl.replaceChildren(...this.deltaContent(ms));
  }

  private deltaContent(ms: number): (Node | string)[] {
    const ghost = this.opts.ghost;
    if (!ghost) return [h("span", { class: "delta__note" }, "No best yet for this mode. This run sets your ghost.")];
    if (!this.ghostOn) return [];
    const best = h("span", { class: "delta__best" }, h("i", { class: "key key--ghost" }), `best ${Math.round(ghost.wpm)}`);
    if (!this.test.started) return [best];
    const diff = Math.round(this.test.pos - this.ghostAt(ms));
    const label =
      diff === 0 ? "level with your best" : diff > 0 ? `${diff} ahead` : `${-diff} behind`;
    const tone = diff > 0 ? "delta__gap--ahead" : diff < 0 ? "delta__gap--behind" : "";
    return [h("span", { class: `delta__gap ${tone}` }, label), best];
  }
}

/** ⌘R restarts instead of reloading the window. */
export const isCmdR = (e: KeyboardEvent): boolean => e.metaKey && e.key.toLowerCase() === "r";

const round2 = (n: number): number => Math.round(n * 100) / 100;
