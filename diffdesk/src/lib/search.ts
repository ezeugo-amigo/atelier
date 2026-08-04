import type { DiffFile } from "./types";

// Find-bar matching and highlighting.
//
// Matches come from the parsed diff model, not the DOM, so a collapsed file
// still contributes results. Highlighting then resolves each match back to a
// DOM range. @pierre/diffs renders code into a shadow root and replaces its
// innerHTML on every render, so injected <mark> wrappers would not survive. The
// CSS Custom Highlight API paints ranges without touching the tree. Its
// `::highlight()` rules must live inside that shadow root, which is what
// `findHighlightCSS` is for (passed to PatchDiff as `options.unsafeCSS`).

const ALL_HIGHLIGHT = "diffdesk-find";
const ACTIVE_HIGHLIGHT = "diffdesk-find-active";

export const findHighlightCSS = `
::highlight(${ALL_HIGHLIGHT}) {
  background-color: #f7dfa0;
  color: #1a1817;
}
::highlight(${ACTIVE_HIGHLIGHT}) {
  background-color: #e09b3c;
  color: #1a1817;
}
`;

type SearchMatchBase = {
  id: string;
  fileId: string;
  filePath: string;
  /** Which occurrence of the needle within this one line/header/path. */
  occurrence: number;
};

export type SearchMatch = SearchMatchBase &
  (
    | { kind: "path" }
    | { kind: "hunk"; hunkIndex: number; hunkHeader: string }
    | { kind: "line"; rowKey: string }
  );

export function buildSearchMatches(
  files: DiffFile[],
  query: string,
): SearchMatch[] {
  const needle = query.trim().toLocaleLowerCase();
  if (needle === "") return [];

  return files.flatMap((file) => {
    const path = filePath(file);
    const matches: SearchMatch[] = [];

    for (const occurrence of occurrences(path, needle)) {
      matches.push({
        kind: "path",
        id: `${file.id}:path:${occurrence}`,
        fileId: file.id,
        filePath: path,
        occurrence,
      });
    }

    file.hunks.forEach((hunk, hunkIndex) => {
      for (const occurrence of occurrences(hunk.header, needle)) {
        matches.push({
          kind: "hunk",
          id: `${file.id}:hunk-${hunkIndex}:${occurrence}`,
          fileId: file.id,
          filePath: path,
          hunkIndex,
          hunkHeader: hunk.header,
          occurrence,
        });
      }

      for (const line of hunk.lines) {
        if (line.kind === "metadata") continue;
        const rowKey = rowKeyForLine(line.kind, line.oldLineNumber, line.newLineNumber);
        if (rowKey === null) continue;
        for (const occurrence of occurrences(line.content, needle)) {
          matches.push({
            kind: "line",
            id: `${file.id}:${rowKey}:${occurrence}`,
            fileId: file.id,
            filePath: path,
            rowKey,
            occurrence,
          });
        }
      }
    });

    return matches;
  });
}

export function applyFindHighlights({
  activeIndex,
  matches,
  pane,
  query,
}: {
  activeIndex: number;
  matches: SearchMatch[];
  pane: HTMLElement | null;
  query: string;
}): void {
  const registry = highlightRegistry();
  const Ctor = highlightConstructor();
  if (registry === null || Ctor === null) return;

  registry.delete(ALL_HIGHLIGHT);
  registry.delete(ACTIVE_HIGHLIGHT);

  const needle = query.trim().toLocaleLowerCase();
  if (pane === null || needle === "" || matches.length === 0) return;

  const contexts = new Map<string, FileContext | null>();
  const rest: Range[] = [];
  let active: Range | null = null;

  matches.forEach((match, index) => {
    let context = contexts.get(match.fileId);
    if (context === undefined) {
      context = fileContext(pane, match.fileId);
      contexts.set(match.fileId, context);
    }
    if (context === null) return;

    const host = hostElement(context, match);
    if (host === null) return;

    const range = rangeForOccurrence(host, needle, match.occurrence);
    if (range === null) return;

    if (index === activeIndex) active = range;
    else rest.push(range);
  });

  if (rest.length > 0) {
    const highlight = new Ctor(...rest);
    highlight.priority = 1;
    registry.set(ALL_HIGHLIGHT, highlight);
  }
  if (active !== null) {
    const highlight = new Ctor(active);
    highlight.priority = 2;
    registry.set(ACTIVE_HIGHLIGHT, highlight);
  }
}

export function scrollMatchIntoView(
  pane: HTMLElement | null,
  match: SearchMatch,
): void {
  if (pane === null) return;
  const context = fileContext(pane, match.fileId);
  if (context === null) return;
  const host = hostElement(context, match);
  (host ?? context.section).scrollIntoView({
    behavior: "smooth",
    block: "center",
    inline: "nearest",
  });
}

/**
 * Identifies a rendered unified row. In unified mode @pierre/diffs emits one row
 * per diff line, carrying `data-line-type` and `data-line` (that side's line
 * number). Deletion rows number against the old file, everything else against
 * the new one, so side plus number is a stable key on both the model side and
 * the DOM side. Reconstructing pierre's own row indices would duplicate its
 * collapsed-context arithmetic and drift from it.
 */
function rowKeyForLine(
  kind: "context" | "addition" | "deletion" | "metadata",
  oldLineNumber: number | null,
  newLineNumber: number | null,
): string | null {
  if (kind === "deletion") {
    return oldLineNumber === null ? null : `old:${oldLineNumber}`;
  }
  return newLineNumber === null ? null : `new:${newLineNumber}`;
}

function rowKeyForElement(element: HTMLElement): string | null {
  const lineNumber = element.dataset.line;
  if (lineNumber === undefined || lineNumber === "") return null;
  const side = element.dataset.lineType?.includes("deletion") === true
    ? "old"
    : "new";
  return `${side}:${lineNumber}`;
}

type FileContext = {
  section: HTMLElement;
  pathElement: HTMLElement | null;
  rows: Map<string, HTMLElement>;
  separators: HTMLElement[];
};

function fileContext(pane: HTMLElement, fileId: string): FileContext | null {
  const section = Array.from(
    pane.querySelectorAll<HTMLElement>("[data-file-id]"),
  ).find((element) => element.dataset.fileId === fileId);
  if (section === undefined) return null;

  const pathElement = section.querySelector<HTMLElement>(".file__path");

  // Rendered code lives in the custom element's shadow root, which a light-DOM
  // querySelectorAll cannot reach.
  const shadowRoot = section.querySelector("diffs-container")?.shadowRoot;
  const rows = new Map<string, HTMLElement>();
  const separators: HTMLElement[] = [];
  if (shadowRoot != null) {
    for (const element of shadowRoot.querySelectorAll<HTMLElement>(
      "[data-content] [data-line-index]",
    )) {
      const key = rowKeyForElement(element);
      if (key !== null && !rows.has(key)) rows.set(key, element);
    }
    separators.push(
      ...shadowRoot.querySelectorAll<HTMLElement>(
        "[data-content] [data-separator]",
      ),
    );
  }

  return { section, pathElement, rows, separators };
}

function hostElement(
  context: FileContext,
  match: SearchMatch,
): HTMLElement | null {
  switch (match.kind) {
    case "path":
      return context.pathElement;
    case "hunk":
      // With `hunkSeparators: "metadata"` each separator renders the raw
      // `@@ … @@` line, so the header text identifies its own separator. Hunks
      // whose separator was not rendered simply find nothing.
      return (
        context.separators.find((element) =>
          (element.textContent ?? "").includes(match.hunkHeader.trim()),
        ) ?? null
      );
    case "line":
      return context.rows.get(match.rowKey) ?? null;
  }
}

function rangeForOccurrence(
  host: Node,
  needle: string,
  occurrence: number,
): Range | null {
  const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT);
  const segments: { node: Text; start: number }[] = [];
  let text = "";
  for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
    const textNode = node as Text;
    segments.push({ node: textNode, start: text.length });
    text += textNode.data;
  }
  if (segments.length === 0) return null;

  const start = nthOccurrenceIndex(text, needle, occurrence);
  if (start === -1) return null;

  const startPoint = pointFor(segments, start);
  const endPoint = pointFor(segments, start + needle.length);
  if (startPoint === null || endPoint === null) return null;

  const range = document.createRange();
  range.setStart(startPoint.node, startPoint.offset);
  range.setEnd(endPoint.node, endPoint.offset);
  return range;
}

function pointFor(
  segments: { node: Text; start: number }[],
  index: number,
): { node: Text; offset: number } | null {
  for (let i = segments.length - 1; i >= 0; i -= 1) {
    const segment = segments[i];
    if (segment === undefined) continue;
    if (index < segment.start) continue;
    const offset = index - segment.start;
    return offset <= segment.node.data.length
      ? { node: segment.node, offset }
      : null;
  }
  return null;
}

/**
 * Indices of every occurrence of `needle` (already lowercased) in `haystack`.
 * Model text and DOM text are scanned by the same rule, so an ordinal taken
 * from one resolves against the other.
 */
function occurrences(haystack: string, needle: string): number[] {
  const lower = haystack.toLocaleLowerCase();
  const result: number[] = [];
  let from = 0;
  for (;;) {
    const index = lower.indexOf(needle, from);
    if (index === -1) return result;
    result.push(result.length);
    from = index + needle.length;
  }
}

function nthOccurrenceIndex(
  haystack: string,
  needle: string,
  occurrence: number,
): number {
  const lower = haystack.toLocaleLowerCase();
  let from = 0;
  for (let seen = 0; ; seen += 1) {
    const index = lower.indexOf(needle, from);
    if (index === -1) return -1;
    if (seen === occurrence) return index;
    from = index + needle.length;
  }
}

// The CSS Custom Highlight API's registry is typed read-only in this project's
// DOM lib, so the two entry points are narrowed by hand.
type HighlightLike = { priority: number };
type HighlightConstructor = new (...ranges: Range[]) => HighlightLike;
type WritableHighlightRegistry = {
  set(name: string, highlight: HighlightLike): void;
  delete(name: string): boolean;
};

function highlightRegistry(): WritableHighlightRegistry | null {
  return (
    (CSS as unknown as { highlights?: WritableHighlightRegistry }).highlights ??
    null
  );
}

function highlightConstructor(): HighlightConstructor | null {
  return (globalThis as { Highlight?: HighlightConstructor }).Highlight ?? null;
}

export function filePath(file: DiffFile): string {
  return file.newPath ?? file.oldPath ?? file.displayPath;
}
