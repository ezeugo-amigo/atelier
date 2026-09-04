// Page zoom for the Today shell.
//
// Tauri doesn't wire the usual browser zoom shortcuts, so we handle them here:
//   Cmd/Ctrl +  (or =)   zoom in
//   Cmd/Ctrl -           zoom out
//   Cmd/Ctrl 0           reset
//
// The whole layout is sized in px, so we scale the document root with CSS
// `zoom` rather than adjusting a root font size. The chosen level is kept in
// localStorage so it survives relaunches.

const STORAGE_KEY = "today.zoom";
const LEVELS = [0.7, 0.8, 0.9, 1, 1.1, 1.25, 1.5, 1.75, 2];
const DEFAULT_INDEX = LEVELS.indexOf(1);

const readStored = () => {
  try {
    const value = Number(localStorage.getItem(STORAGE_KEY));
    const index = LEVELS.indexOf(value);
    return index === -1 ? DEFAULT_INDEX : index;
  } catch {
    return DEFAULT_INDEX;
  }
};

const store = (level) => {
  try {
    if (level === 1) localStorage.removeItem(STORAGE_KEY);
    else localStorage.setItem(STORAGE_KEY, String(level));
  } catch {
    // Storage unavailable; zoom still applies for this session.
  }
};

export function wireZoom({ onChange } = {}) {
  let index = readStored();

  const apply = () => {
    const level = LEVELS[index];
    document.documentElement.style.zoom = level === 1 ? "" : String(level);
    store(level);
    if (onChange) onChange(level);
  };

  const step = (delta) => {
    const next = Math.min(LEVELS.length - 1, Math.max(0, index + delta));
    if (next === index) return;
    index = next;
    apply();
  };

  const reset = () => {
    if (index === DEFAULT_INDEX) return;
    index = DEFAULT_INDEX;
    apply();
  };

  document.addEventListener("keydown", (event) => {
    const modifier = event.metaKey || event.ctrlKey;
    if (!modifier || event.altKey) return;

    // `key` varies with layout and Shift ("=" vs "+"); check `code` too so the
    // shortcut works on every keyboard, including the numeric keypad.
    const key = event.key;
    const code = event.code;

    if (key === "+" || key === "=" || code === "Equal" || code === "NumpadAdd") {
      event.preventDefault();
      step(1);
    } else if (key === "-" || key === "_" || code === "Minus" || code === "NumpadSubtract") {
      event.preventDefault();
      step(-1);
    } else if (key === "0" || code === "Digit0" || code === "Numpad0") {
      event.preventDefault();
      reset();
    }
  });

  apply();
}
