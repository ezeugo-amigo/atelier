# Lotus design system v0.1

Cool neutrals · mono chrome · serif editorial voice · radius 0.

Source of truth: the "Lotus Restyle" Claude Design project (turn 4b). Implemented as
CSS custom properties in `web/styles/app.css`.

## Principles

1. The number to zero is always visible.
2. The agent speaks in serif, the system in mono.
3. One green; it means progress.
4. Chrome is grey, mail is white.
5. Nothing sends without you.

## Color — 9 neutrals, 2 signals

| Token | Hex | Use |
|---|---|---|
| `--paper` | `#fafbfb` | app background |
| `--surface` | `#ffffff` | rows, cards, mail |
| `--mist` | `#f5f6f7` | wells, side panels |
| `--hairline` | `#eceef0` | row dividers |
| `--line` | `#e3e6e8` | panel borders, outlines |
| `--faint` | `#b6bcc1` | disabled, tertiary hints |
| `--muted` | `#9ba2a8` | meta, timestamps |
| `--secondary` | `#6a727a` | read mail, quiet labels |
| `--body` | `#2c3238` | reading text |
| `--ink` | `#16181b` | headings, primary actions |
| `--signal` | `#3d9970` | unread, agent, progress — the only green |
| `--flag` | `#c2643f` | urgent, sparingly |

Tints: `--signal-tint #f6f9f7`, `--flag-tint #fdf9f7`. `--key-line #4a4f55` outlines
keyboard hints on ink surfaces.

## Type — mono is the interface, serif is the voice

- UI + mail content: JetBrains Mono (`--font-mono`). Body renders at 12px / 1.75.
- Agent / editorial voice: Newsreader (`--font-serif`), often italic, 15–18px prose.
- Headlines: mono 600, 20–30px, −2% tracking, lowercase.
- Section labels: mono 400, 10px, +6% tracking, muted, prefixed `// `.
- Counters: mono 600, up to 88px in situ.

## Actions — square, bordered, always with a key

- Radius 0 everywhere.
- Primary (solid ink) = the one default action.
- Secondary = 1px ink border; quiet = 1px `--line` border, secondary text.
- Solid signal green = the agent acting on your behalf.
- Destructive actions are never solid.
- Every action shows its keyboard shortcut (bordered kbd hint).

## Parts

- **List row** — white surface on paper, 1px hairline between rows. Unread =
  signal dot + weight 600 ink; read = secondary. Selected = mist + 2px ink inset.
- **Ledger line** — heavy ink rule on top, hairlines within; typed markers ✓ ✎ ⚑ ＋ ∅.
- **Progress** — green segments done, ink current, `--line` remaining.
- **Panel headings** — 2px ink rule under the title (see setup panels, composer).
