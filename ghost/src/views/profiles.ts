// "Who's typing?" — pick a profile, unlock it with its PIN if it has one, or add a new one.

import { h } from "../dom";
import { checkPin, hashPin, newProfile, type Profile } from "../store";

export interface ProfilesInput {
  profiles: Profile[];
  onPick: (profile: Profile) => void;
  onCreate: (profile: Profile) => void;
}

export function profilesView({ profiles, onPick, onCreate }: ProfilesInput): HTMLElement {
  const list = h("ul", { class: "people" });

  for (const p of profiles) {
    const best = Math.max(0, ...Object.values(p.bests).map((b) => b.wpm));
    const meta = p.results.length
      ? `best ${Math.round(best)} wpm · ${p.results.length} ${p.results.length === 1 ? "test" : "tests"}`
      : "no tests yet";
    const row = h("li", { class: "person" });
    const name = h("button", { class: "person__name" }, p.name, p.pin ? h("span", { class: "person__lock" }, "PIN") : null);
    name.addEventListener("click", () => {
      if (!p.pin) return onPick(p);
      if (row.querySelector("form")) return;
      row.append(pinForm(p, onPick));
    });
    row.append(name, h("span", { class: "person__meta" }, meta));
    list.append(row);
  }

  const add = h("li", { class: "person person--add" });
  const addButton = h("button", { class: "person__name person__name--add" }, profiles.length ? "Add someone" : "Add yourself");
  addButton.addEventListener("click", () => {
    add.replaceChildren(createForm(onCreate, profiles));
    add.querySelector("input")?.focus();
  });
  add.append(addButton);
  list.append(add);

  return h(
    "main",
    { class: "profiles" },
    h("h1", { class: "display" }, "Who's typing?"),
    h("p", { class: "lede" }, "Every profile keeps its own results, and races its own best run."),
    list,
  );
}

function pinForm(p: Profile, onPick: (p: Profile) => void): HTMLElement {
  const input = h("input", { class: "field", type: "password", inputmode: "numeric", placeholder: "PIN", autocomplete: "off" });
  const error = h("span", { class: "form__error" });
  const form = h("form", { class: "form form--inline" }, input, h("button", { class: "button" }, "Unlock"), error);
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    if (await checkPin(p, input.value)) onPick(p);
    else {
      error.textContent = "That PIN doesn't match.";
      input.select();
    }
  });
  queueMicrotask(() => input.focus());
  return form;
}

function createForm(onCreate: (p: Profile) => void, existing: Profile[]): HTMLElement {
  const name = h("input", { class: "field field--name", placeholder: "Name", maxlength: 32, autocomplete: "off" });
  const pin = h("input", {
    class: "field",
    type: "password",
    inputmode: "numeric",
    placeholder: "PIN (optional)",
    maxlength: 12,
    autocomplete: "off",
  });
  const error = h("span", { class: "form__error" });
  const form = h(
    "form",
    { class: "form" },
    name,
    pin,
    h("button", { class: "button" }, "Create profile"),
    error,
  );
  form.addEventListener("submit", async (e) => {
    e.preventDefault();
    const trimmed = name.value.trim();
    if (!trimmed) return void (error.textContent = "Give the profile a name.");
    if (existing.some((p) => p.name.toLowerCase() === trimmed.toLowerCase()))
      return void (error.textContent = "Someone already has that name.");
    const profile = newProfile(trimmed);
    if (pin.value) profile.pin = await hashPin(pin.value);
    onCreate(profile);
  });
  return form;
}
