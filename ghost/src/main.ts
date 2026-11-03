// Ghost — app shell: the header, which screen is showing, and saving results.

import "./style.css";
import { h } from "./dom";
import { loadDb, MODE_VALUES, modeKey, modeLabel, recordResult, saveDb, type Db, type Mode, type ModeKind, type Profile } from "./store";
import { profileView } from "./views/profile";
import { profilesView } from "./views/profiles";
import { resultsView } from "./views/results";
import { isCmdR, TestView } from "./views/test";

/** Runs this short or this sloppy aren't worth keeping (or becoming a ghost). */
const MIN_KEYSTROKES = 5;

type Screen = "profiles" | "test" | "results" | "profile";

class App {
  private profile: Profile | null = null;
  private screen: Screen = "profiles";
  private test: TestView | null = null;
  private header = h("header", { class: "bar", "data-tauri-drag-region": true });
  private body = h("div", { class: "screen" });

  constructor(private db: Db) {
    document.getElementById("app")!.append(this.header, this.body);
    document.addEventListener("keydown", (e) => this.onKey(e));
    // Buttons keep focus after a click; drop it so Space and Enter go to the test instead.
    document.addEventListener("click", (e) => {
      if ((e.target as HTMLElement).closest("button")) (document.activeElement as HTMLElement | null)?.blur();
    });
    // Moving the mouse brings the header back while typing.
    document.addEventListener("mousemove", () => document.body.classList.remove("typing"));
    if (db.profiles.length === 1 && !db.profiles[0].pin) this.signIn(db.profiles[0]);
    else this.show("profiles");
  }

  private save(): void {
    void saveDb(this.db);
  }

  private signIn(profile: Profile): void {
    this.profile = profile;
    this.show("test");
  }

  private show(screen: Screen): void {
    this.test?.destroy();
    this.test = null;
    if (screen === "profiles") this.profile = null;
    const p = this.profile;
    this.screen = p ? screen : "profiles";
    document.documentElement.dataset.theme = p?.settings.theme ?? "paper";
    this.renderHeader();
    if (!p) {
      this.body.replaceChildren(
        profilesView({
          profiles: this.db.profiles,
          onPick: (picked) => this.signIn(picked),
          onCreate: (created) => {
            this.db.profiles.push(created);
            this.save();
            this.signIn(created);
          },
        }),
      );
    } else if (screen === "test") {
      this.startTest(p);
    } else if (screen === "profile") {
      this.body.replaceChildren(
        profileView({
          profile: p,
          onChange: () => {
            this.save();
            document.documentElement.dataset.theme = p.settings.theme;
          },
          onPlay: (mode) => this.setMode(mode),
          onDelete: () => {
            this.db.profiles = this.db.profiles.filter((x) => x !== p);
            this.save();
            this.show("profiles");
          },
        }),
      );
    }
  }

  private startTest(p: Profile): void {
    const mode = p.settings.mode;
    const best = p.bests[modeKey(mode)] ?? null;
    this.test = new TestView({
      mode,
      ghost: best,
      ghostStyle: p.settings.ghost,
      onFinish: (run) => {
        if (run.chars.correct + run.chars.incorrect + run.chars.extra < MIN_KEYSTROKES || run.wpm <= 0) {
          this.show("test");
          return;
        }
        const replaced = recordResult(p, run);
        this.save();
        this.screen = "results";
        this.test = null;
        this.renderHeader();
        this.body.replaceChildren(
          resultsView({ run, modeLabel: modeLabel(mode), raced: best, isBest: replaced !== undefined }),
        );
      },
    });
    this.body.replaceChildren(this.test.el);
  }

  private setMode(mode: Mode): void {
    if (!this.profile) return;
    this.profile.settings.mode = mode;
    this.save();
    this.show("test");
  }

  private onKey(e: KeyboardEvent): void {
    if (this.screen === "test" && this.test) {
      if ((e.target as HTMLElement).closest("input")) return;
      this.test.onKey(e);
    } else if (this.screen === "results") {
      // Keys that spill over from the end of a test shouldn't scroll the page.
      const next = e.key === "Tab" || e.key === "Enter" || isCmdR(e);
      if (next || e.key === " ") e.preventDefault();
      if (next) this.show("test");
    } else if (this.screen === "profile" && e.key === "Escape") {
      this.show("test");
    }
  }

  private renderHeader(): void {
    const p = this.profile;
    const brand = h("button", { class: "brand" }, "Ghost");
    brand.addEventListener("click", () => this.show(p ? "test" : "profiles"));
    if (!p) {
      this.header.replaceChildren(brand);
      return;
    }
    const mode = p.settings.mode;
    const modes = h("nav", { class: "modes" });
    for (const kind of Object.keys(MODE_VALUES) as ModeKind[]) {
      const b = h("button", { class: `modes__opt${mode.kind === kind ? " is-on" : ""}` }, kind);
      b.addEventListener("click", () => this.setMode({ kind, value: MODE_VALUES[kind][1] }));
      modes.append(b);
    }
    modes.append(h("span", { class: "modes__sep" }));
    for (const value of MODE_VALUES[mode.kind]) {
      const b = h("button", { class: `modes__opt${mode.value === value ? " is-on" : ""}` }, String(value));
      b.addEventListener("click", () => this.setMode({ kind: mode.kind, value }));
      modes.append(b);
    }
    const who = h("button", { class: `who${this.screen === "profile" ? " is-on" : ""}` }, p.name);
    who.addEventListener("click", () => this.show(this.screen === "profile" ? "test" : "profile"));
    const swap = h("button", { class: "who who--quiet" }, "switch");
    swap.addEventListener("click", () => this.show("profiles"));
    this.header.replaceChildren(brand, modes, h("div", { class: "bar__right" }, who, swap));
  }
}

loadDb().then((db) => new App(db));
