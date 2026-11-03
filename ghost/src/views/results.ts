// After a test: the number, how it compares with the best you raced, and the curve.

import { speedChart, type Series } from "../chart";
import { h } from "../dom";
import type { Best, Result } from "../store";

export interface ResultsInput {
  run: Result;
  modeLabel: string;
  /** The best this run was raced against, if there was one. */
  raced: Best | null;
  /** True when this run became the new best (and so the new ghost). */
  isBest: boolean;
}

export function resultsView({ run, modeLabel, raced, isBest }: ResultsInput): HTMLElement {
  const wpm = Math.round(run.wpm);
  let verdict: string;
  if (!raced) verdict = "Your first run in this mode. It's your ghost from now on.";
  else if (isBest) verdict = `A new best, ${fmtGap(run.wpm - raced.wpm)} faster than your ghost. It races you next time.`;
  else verdict = `${fmtGap(raced.wpm - run.wpm)} short of your best of ${Math.round(raced.wpm)}.`;

  const series: Series[] = [{ name: "this run", color: "--you", values: run.samples }];
  if (raced) series.push({ name: "your best", color: "--ghost", values: raced.samples });

  const { correct, incorrect, extra, missed } = run.chars;
  return h(
    "main",
    { class: "results" },
    h(
      "section",
      { class: "results__head" },
      h("p", { class: "eyebrow" }, isBest && raced ? "New personal best" : modeLabel),
      h("h1", { class: "results__wpm" }, String(wpm), h("span", { class: "results__unit" }, "wpm")),
      h("p", { class: "results__verdict" }, verdict),
    ),
    h(
      "dl",
      { class: "stats" },
      stat("accuracy", `${Math.round(run.acc)}%`),
      stat("raw", String(Math.round(run.raw))),
      stat("characters", `${correct}/${incorrect}/${extra}/${missed}`, "correct / incorrect / extra / missed"),
      stat("time", `${(run.ms / 1000).toFixed(1)}s`),
      stat("mode", modeLabel),
    ),
    speedChart(series),
    h("p", { class: "hint" }, h("kbd", null, "tab"), " or ", h("kbd", null, "enter"), " next test"),
  );
}

function stat(label: string, value: string, title?: string): HTMLElement {
  return h("div", { class: "stat", title }, h("dt", null, label), h("dd", null, value));
}

function fmtGap(n: number): string {
  const v = Math.abs(n);
  return `${v < 1 ? v.toFixed(1) : Math.round(v)} wpm`;
}
