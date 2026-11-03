// Speed over the run: this attempt against the best it was racing, one point per second.

import { h, s } from "./dom";

export interface Series {
  name: string;
  /** CSS custom property that colors the line, e.g. "--you". */
  color: string;
  values: number[];
}

const W = 720;
const H = 190;
const PAD = { top: 16, right: 56, bottom: 28, left: 36 };

export function speedChart(series: Series[]): HTMLElement {
  const plotted = series.filter((x) => x.values.length > 0);
  const n = Math.max(...plotted.map((x) => x.values.length));
  const top = Math.max(...plotted.flatMap((x) => x.values));
  const step = top > 150 ? 50 : 25;
  const yMax = Math.max(step * 2, Math.ceil(top / step) * step);
  const innerW = W - PAD.left - PAD.right;
  const innerH = H - PAD.top - PAD.bottom;
  const x = (i: number) => PAD.left + (n <= 1 ? innerW : (i / (n - 1)) * innerW);
  const y = (v: number) => PAD.top + innerH - (v / yMax) * innerH;

  const svg = s("svg", { viewBox: `0 0 ${W} ${H}`, class: "chart__svg", role: "img" });
  svg.append(s("title", {}, `Words per minute by second: ${plotted.map((p) => p.name).join(" vs ")}`));

  for (let v = 0; v <= yMax; v += step) {
    svg.append(s("line", { x1: PAD.left, x2: W - PAD.right, y1: y(v), y2: y(v), class: "chart__grid" }));
    svg.append(s("text", { x: PAD.left - 8, y: y(v) + 4, class: "chart__tick", "text-anchor": "end" }, String(v)));
  }
  const xEvery = n > 60 ? 15 : n > 20 ? 5 : n > 8 ? 2 : 1;
  for (let i = 0; i < n; i++) {
    if ((i + 1) % xEvery !== 0) continue;
    svg.append(s("text", { x: x(i), y: H - 8, class: "chart__tick", "text-anchor": "middle" }, `${i + 1}s`));
  }

  // Draw the comparison first so this run sits on top.
  for (const ser of [...plotted].reverse()) {
    const d = ser.values.map((v, i) => `${i === 0 ? "M" : "L"}${x(i).toFixed(1)},${y(v).toFixed(1)}`).join("");
    svg.append(s("path", { d, class: "chart__line", style: `stroke: var(${ser.color})` }));
    const last = ser.values.length - 1;
    svg.append(
      s("circle", { cx: x(last), cy: y(ser.values[last]), r: 4, class: "chart__dot", style: `fill: var(${ser.color})` }),
    );
  }
  // End labels, nudged apart when the two runs finish close together.
  const ends = plotted.map((ser) => ({ ser, ly: y(ser.values[ser.values.length - 1]) }));
  if (ends.length === 2 && Math.abs(ends[0].ly - ends[1].ly) < 16) {
    const [hi, lo] = ends[0].ly < ends[1].ly ? [ends[0], ends[1]] : [ends[1], ends[0]];
    const mid = (hi.ly + lo.ly) / 2;
    hi.ly = mid - 8;
    lo.ly = mid + 8;
  }
  for (const { ser, ly } of ends) {
    const last = ser.values.length - 1;
    svg.append(
      s("text", { x: x(last) + 10, y: ly + 4, class: "chart__end" }, String(Math.round(ser.values[last]))),
    );
  }

  // Hover: a crosshair and a readout of every series at that second.
  const cross = s("line", { y1: PAD.top, y2: PAD.top + innerH, class: "chart__cross" });
  cross.style.display = "none";
  svg.append(cross);
  const tip = h("div", { class: "chart__tip" });
  tip.hidden = true;
  const hit = s("rect", { x: PAD.left, y: 0, width: innerW, height: H, fill: "transparent" });
  svg.append(hit);
  const fig = h("figure", { class: "chart" });

  hit.addEventListener("mousemove", (e) => {
    const box = svg.getBoundingClientRect();
    const px = ((e as MouseEvent).clientX - box.left) * (W / box.width);
    const i = Math.max(0, Math.min(n - 1, Math.round(((px - PAD.left) / innerW) * (n - 1))));
    cross.setAttribute("x1", String(x(i)));
    cross.setAttribute("x2", String(x(i)));
    cross.style.display = "";
    tip.replaceChildren(
      h("div", { class: "chart__tip-title" }, `${i + 1}s`),
      ...plotted.map((ser) =>
        h(
          "div",
          { class: "chart__tip-row" },
          h("i", { class: "key", style: `background: var(${ser.color})` }),
          ser.name,
          h("b", null, ser.values[i] === undefined ? "–" : String(Math.round(ser.values[i]))),
        ),
      ),
    );
    tip.hidden = false;
    const left = (x(i) / W) * box.width;
    tip.style.left = `${Math.min(left + 12, box.width - 150)}px`;
  });
  hit.addEventListener("mouseleave", () => {
    cross.style.display = "none";
    tip.hidden = true;
  });

  const legend =
    plotted.length > 1
      ? h(
          "div",
          { class: "chart__legend" },
          ...plotted.map((ser) =>
            h("span", null, h("i", { class: "key key--line", style: `background: var(${ser.color})` }), ser.name),
          ),
        )
      : null;

  fig.append(h("figcaption", { class: "chart__caption" }, h("span", null, "wpm over time"), legend), svg, tip);
  return fig;
}
