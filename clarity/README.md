# Clarity

> A quiet, keyboard-first Markdown editor for macOS, in the spirit of iA Writer:
> a native Swift app that opens a folder of notes and gets out of the way.

Clarity is one centered column of monospaced text on a dark page. Markdown
syntax stays visible but fades back, heading hashes hang in the left margin so
titles line up with body text, and everything you'd reach for the mouse to do
has a shortcut. Point it at any folder of `.md` files, including an Obsidian
vault, and it reads and writes them in place.

## Features

- **Minimal by default.** Dark mode, [iA Writer Mono](https://github.com/iaolo/iA-Fonts)
  (bundled), an 80-character centered column (adjustable from 40 to 200 with
  ⌥⌘= / ⌥⌘-), generous line height, a hidden
  title bar, and a faint word count. The mouse pointer hides while you type.
- **Focus mode** (⌘D) dims everything but the current sentence, keeps the
  caret line in the middle of the window, and hides the sidebar.
- **GitHub-flavored Markdown**: headings, bold/italic, strikethrough, inline and
  fenced code, blockquotes, lists, task lists, tables, footnotes, links, bare URLs.
- **Obsidian syntax**: `[[wikilinks]]`, `[[note|aliases]]`, `![[embeds]]`,
  `#tags`, `==highlights==`, `> [!note]` callouts, `%% comments %%`, and YAML
  frontmatter.
- **Links you can follow.** ⌘-click or ⌘↩ on a wikilink or Markdown link opens
  the note (by relative path, or by name anywhere in the folder). A link to a
  note that doesn't exist yet creates it, as in Obsidian. Web links open in the browser.
- **Writing helpers.** Return continues lists, numbered lists, task lists and
  quotes; Return on an empty item ends the list. Tab / ⇧Tab indent list items.
- **Tables.** Type `/table` (or `/table 4x3` for 4 columns × 3 rows) on its own
  line and press Return or Tab to get a Markdown table. Inside a table, Tab /
  ⇧Tab move between cells and re-align the columns, Tab past the last cell adds a
  row, Return adds a row below, Return on an empty row leaves the table, and ⌘↩
  breaks a line within a cell. The break shows as a real new line under the
  cell and is saved as `<br>`, since a Markdown table row can't span lines.
  Format ▸ Table adds and deletes rows and columns. It's all plain GitHub-style
  Markdown on disk.
- **Autosave.** Notes save half a second after you stop typing, and when the app
  loses focus. Changes made on disk by other tools (git, sync, another editor)
  are picked up when you switch back.
- **Edit history.** Every edit and caret move to a note is logged, so how it was
  written can be replayed later. File ▸ Export History… saves it in the
  [ezeugo.dev](https://ezeugo.dev) replay format.

## Keyboard

| Shortcut | Action |
| --- | --- |
| ⌘N | New note |
| ⌘O / ⌘P | Open note: fuzzy-find by name or path; Return opens, or creates the typed name |
| ⇧⌘O | Open folder… |
| ⌘[ / ⌘] | Back / forward through the notes you've visited |
| ⌥⌘↑ / ⌥⌘↓ | Previous / next note in the sidebar |
| ⌘↩ | Follow the link under the caret (in a table, inserts a line break instead) |
| ⌘\\ | Toggle sidebar |
| ⌘D | Focus mode |
| ⇧⌘D | Toggle dark / light |
| ⌘= / ⌘- / ⌘0 | Bigger / smaller / default text size |
| ⌥⌘= / ⌥⌘- / ⌥⌘0 | Wider / narrower / default writing column (10 characters per step) |
| ⌘B / ⌘I | Bold / italic (toggles) |
| ⇧⌘X / ⇧⌘H / ⌥⌘C | Strikethrough / highlight / inline code |
| ⌘K | Link: wraps the selection as `[text]()`, or inserts `[[]]` |
| ⌘L | Toggle a task checkbox on the current line |
| ⇧⌘C | Copy as rich text (the selection, or the whole note): pastes into Notion with tables intact |
| ⌥⌘T | Insert table (or type `/table`) |
| Tab / ⇧Tab | In a table: next / previous cell |
| ⌘↩ | In a table: line break within the cell (`<br>`) |
| ⌥⌘↩ | In a table: add row |
| ⌥⌘← / ⌥⌘→ | In a table: add column left / right |
| ⌥⌘⌫ / ⇧⌥⌘⌫ | In a table: delete row / delete column |
| ⇧⌘R / ⌥⌘R / ⇧⌘⌫ | Rename / reveal in Finder / move to Trash |
| ⌘F, ⌥⌘F, ⌘G | Find, find and replace, find next |
| ⌃⌘F | Full screen |

## Run it

Clarity is a Swift package and builds with just the Xcode Command Line Tools
(Swift 5.10+, macOS 14+). No Xcode project is needed. From this `clarity/` directory:

```sh
make install    # build Clarity.app and copy it into /Applications
make launch     # open the installed app (installs it first if needed)
make run        # build + run the debug binary directly (make run ARGS=~/notes)
make build      # just build build/Clarity.app
make help       # list all targets
```

On first launch Clarity opens `~/Documents/Clarity`, creating it if needed.
Use ⇧⌘O to choose another folder; Clarity remembers it. You can also hand it a
folder or file directly:

```sh
open -a Clarity ~/notes            # a folder becomes the workspace
open -a Clarity ~/notes/today.md   # a file opens in its folder
```

## How it's built

```
clarity/
├── Package.swift              SwiftPM executable target (macOS 14+), depends on swift-markdown
├── Makefile                   build/install: assembles Clarity.app around the release binary
├── scripts/make-icon.swift    draws the app icon (`make icon` regenerates it)
├── Resources/
│   ├── Info.plist             bundle metadata; registers as an editor for .md and folders
│   ├── AppIcon.icns           app icon (AppIcon.png is the 1024px preview)
│   └── Fonts/                 iA Writer Mono S (SIL OFL; see Fonts/LICENSE.md)
└── Sources/Clarity/
    ├── AppDelegate.swift      entry point, window, menus and every keyboard shortcut
    ├── Workspace.swift        the folder of notes: open, autosave, history, links, rename/trash
    ├── OpLog.swift            each note's edit log: record, reconcile, store, export
    ├── EditorTextView.swift   the NSTextView: column layout, focus mode, list editing, commands
    ├── EditorTextView+Tables.swift  /table, cell navigation, row/column commands
    ├── MarkdownTable.swift    parse a GFM table, edit rows/columns, render it aligned
    ├── MarkdownStyler.swift   regex-based Markdown styling applied as text attributes
    ├── MarkdownHTML.swift     Markdown to HTML for Copy as Rich Text (parsed with swift-markdown)
    ├── ContentView.swift      SwiftUI layout: sidebar + editor + title + word count
    ├── Sidebar.swift          note list, most recently modified first
    ├── QuickOpen.swift        the ⌘O palette
    └── Theme.swift            colors (dark + light) and font loading
```

**AppKit for the text, SwiftUI for the chrome.** The editor is an `NSTextView`
on a hand-built TextKit 1 stack, because a writing app needs control over
layout and styling that SwiftUI's `TextEditor` doesn't give. The window, menus
and app lifecycle are plain AppKit; the sidebar and palette are SwiftUI views
that read an `@Observable` `Workspace`.

**Styling is just attributes.** The file on disk is exactly what you typed.
After every edit, `MarkdownStyler` scans the document's block structure
(fences, frontmatter, comments) but only restyles the paragraphs around the
edit, falling back to a full pass when that structure changes. Restyling
everything would make AppKit re-estimate the height of off-screen text on
every keystroke, which visibly shifts the view. Headings are bold and scale by
level (`#` 1.6×, `##` 1.35×, `###` 1.15×, deeper levels at body size). Every
paragraph is indented by a gutter the width of `###### `; headings shrink their
first-line indent by their marker's width at the heading's size, so the `#`s
hang in that gutter and the title text lines up with the body.

**Focus mode doesn't touch the document.** Dimming uses layout-manager
temporary attributes, which override colors at draw time without editing the
text storage, so undo and autosave never see it.

**Line breaks in table cells.** On disk a break inside a cell is `<br>`; in
the editor it's U+2028 LINE SEPARATOR, swapped on load and save (and on copy).
That character is a real line break that stays inside the row's paragraph, so
the text system wraps it under the cell's column and the caret treats it as
one character, with no layout tricks.

**History is an op log, not snapshots.** Each note gets a JSON Lines file in
`~/Library/Application Support/Clarity/History` (named by a hash of the note's
path) with one entry per change: `{"t":…,"at":12,"del":"🎉","ins":"!"}` for an
edit, `{"t":…,"sel":[0,5]}` for a caret move that an edit didn't imply. Offsets
are UTF-16 units into the editor's text. Edits are recorded in the text
storage's `didProcessEditing`, the one place every change passes (undo and redo
skip `shouldChangeText`), and the log keeps its own copy of the document to
recover what each edit replaced. When that copy disagrees with the note
(it was changed on disk, or the app quit before a flush), one edit covering the
difference is appended, so replaying from an empty page always ends at the
current text. Logs follow renames and are deleted with the note. A note you
only read never gets one.

**The folder is the database.** There's no index. Notes are listed by scanning
the folder (hidden folders like `.obsidian` and `.git` are skipped), wikilinks
are resolved against that list at click time, and renaming a note does not
rewrite links that point to it.
