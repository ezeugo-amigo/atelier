// One profile: personal bests per mode, recent results, and settings.

import { h } from "../dom";
import type { GhostStyle } from "../ghost";
import { hashPin, MODE_VALUES, modeLabel, type Mode, type ModeKind, type Profile, type Theme } from "../store";

export interface ProfileInput {
  profile: Profile;
  onChange: () => void;
  onPlay: (mode: Mode) => void;
  onDelete: () => void;
}

const RECENT = 12;

export function profileView({ profile, onChange, onPlay, onDelete }: ProfileInput): HTMLElement {
  const since = new Date(profile.createdAt).toLocaleDateString(undefined, { month: "long", year: "numeric" });
  const count = profile.results.length;

  const bests = h("div", { class: "bests" });
  for (const kind of Object.keys(MODE_VALUES) as ModeKind[]) {
    for (const value of MODE_VALUES[kind]) {
      const mode = { kind, value };
      const best = profile.bests[`${kind}-${value}`];
      const cell = h(
        "button",
        { class: `best${best ? "" : " best--empty"}`, title: `Play ${modeLabel(mode)}` },
        h("span", { class: "best__mode" }, modeLabel(mode)),
        h("span", { class: "best__wpm" }, best ? String(Math.round(best.wpm)) : "–"),
        h("span", { class: "best__acc" }, best ? `${Math.round(best.acc)}% acc` : "no ghost yet"),
      );
      cell.addEventListener("click", () => onPlay(mode));
      bests.append(cell);
    }
  }

  const recent = profile.results.slice(-RECENT).reverse();
  const table = recent.length
    ? h(
        "table",
        { class: "recent" },
        h("thead", null, h("tr", null, h("th", null, "when"), h("th", null, "mode"), h("th", null, "wpm"), h("th", null, "acc"))),
        h(
          "tbody",
          null,
          ...recent.map((r) => {
            const [kind, value] = r.mode.split("-");
            return h(
              "tr",
              null,
              h("td", null, when(r.at)),
              h("td", null, modeLabel({ kind: kind as ModeKind, value: Number(value) })),
              h("td", { class: "num" }, String(Math.round(r.wpm))),
              h("td", { class: "num" }, `${Math.round(r.acc)}%`),
            );
          }),
        ),
      )
    : h("p", { class: "muted" }, "Nothing yet. Your results show up here.");

  const settings = profile.settings;
  const ghostChoice = choice<GhostStyle>(
    [
      ["replay", "replay my best"],
      ["steady", "steady pace"],
      ["off", "off"],
    ],
    settings.ghost,
    (v) => {
      settings.ghost = v;
      onChange();
    },
  );
  const themeChoice = choice<Theme>(
    [
      ["paper", "paper"],
      ["cocoa", "cocoa"],
    ],
    settings.theme,
    (v) => {
      settings.theme = v;
      onChange();
    },
  );

  return h(
    "main",
    { class: "profile" },
    h("p", { class: "eyebrow" }, `Typing since ${since}`),
    h("h1", { class: "display" }, profile.name),
    h("p", { class: "lede" }, count ? `${count} ${count === 1 ? "test" : "tests"} so far.` : "No tests yet."),
    h("h2", { class: "section" }, "Personal bests"),
    bests,
    h("h2", { class: "section" }, "Recent"),
    table,
    h("h2", { class: "section" }, "Settings"),
    h(
      "dl",
      { class: "settings" },
      h("dt", null, "Ghost"),
      h("dd", null, ghostChoice),
      h("dt", null, "Theme"),
      h("dd", null, themeChoice),
      h("dt", null, "PIN"),
      h("dd", null, pinControl(profile, onChange)),
      h("dt", null, "Profile"),
      h("dd", null, deleteControl(onDelete)),
    ),
  );
}

function choice<T extends string>(options: [T, string][], current: T, onPick: (v: T) => void): HTMLElement {
  const wrap = h("div", { class: "choice" });
  for (const [value, label] of options) {
    const b = h("button", { class: `choice__opt${value === current ? " is-on" : ""}` }, label);
    b.addEventListener("click", () => {
      wrap.querySelectorAll(".choice__opt").forEach((el) => el.classList.remove("is-on"));
      b.classList.add("is-on");
      onPick(value);
    });
    wrap.append(b);
  }
  return wrap;
}

function pinControl(profile: Profile, onChange: () => void): HTMLElement {
  const wrap = h("div", { class: "choice" });
  const render = () => {
    const set = h("button", { class: "choice__opt" }, profile.pin ? "change PIN" : "set a PIN");
    set.addEventListener("click", () => {
      const input = h("input", { class: "field", type: "password", inputmode: "numeric", placeholder: "New PIN", maxlength: 12 });
      const form = h("form", { class: "form form--inline" }, input, h("button", { class: "button" }, "Save"));
      form.addEventListener("submit", async (e) => {
        e.preventDefault();
        if (!input.value) return;
        profile.pin = await hashPin(input.value);
        onChange();
        render();
      });
      wrap.replaceChildren(form);
      input.focus();
    });
    wrap.replaceChildren(set);
    if (profile.pin) {
      const remove = h("button", { class: "choice__opt" }, "remove PIN");
      remove.addEventListener("click", () => {
        delete profile.pin;
        onChange();
        render();
      });
      wrap.append(remove);
    }
  };
  render();
  return wrap;
}

function deleteControl(onDelete: () => void): HTMLElement {
  const b = h("button", { class: "choice__opt choice__opt--danger" }, "delete this profile");
  let armed = false;
  b.addEventListener("click", () => {
    if (armed) return onDelete();
    armed = true;
    b.textContent = "click again to delete it and every result";
    setTimeout(() => {
      armed = false;
      b.textContent = "delete this profile";
    }, 4000);
  });
  return b;
}

function when(iso: string): string {
  const d = new Date(iso);
  const today = new Date();
  const sameDay = d.toDateString() === today.toDateString();
  return sameDay
    ? d.toLocaleTimeString(undefined, { hour: "numeric", minute: "2-digit" })
    : d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
