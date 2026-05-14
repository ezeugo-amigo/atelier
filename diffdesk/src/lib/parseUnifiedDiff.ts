import type {
  DiffFile,
  DiffFileStatus,
  DiffHunk,
  DiffLine,
  DiffLineKind,
} from "./types";

const hunkPattern = /^@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@(.*)$/;

export function parseUnifiedDiff(rawDiff: string): DiffFile[] {
  const lines = rawDiff.replaceAll("\r\n", "\n").split("\n");
  const files: DiffFile[] = [];
  let file: DiffFile | null = null;
  let hunk: DiffHunk | null = null;
  let oldLine = 0;
  let newLine = 0;

  const pushFile = () => {
    if (file === null) return;
    file.additions = file.hunks.reduce(
      (total, current) =>
        total + current.lines.filter((line) => line.kind === "addition").length,
      0,
    );
    file.deletions = file.hunks.reduce(
      (total, current) =>
        total + current.lines.filter((line) => line.kind === "deletion").length,
      0,
    );
    file.tooLarge = file.additions + file.deletions > 2000;
    files.push(file);
  };

  const ensureFile = () => {
    if (file !== null) return file;
    file = createFile(`file-${files.length}`, null, null);
    return file;
  };

  for (const rawLine of lines) {
    if (rawLine.startsWith("diff --git ")) {
      pushFile();
      const parsed = parseGitDiffHeader(rawLine);
      file = createFile(`file-${files.length}`, parsed.oldPath, parsed.newPath);
      hunk = null;
      continue;
    }

    if (file === null && rawLine.trim() === "") {
      continue;
    }

    if (rawLine.startsWith("--- ")) {
      const current = ensureFile();
      current.oldPath = normalizeDiffPath(rawLine.slice(4));
      current.displayPath =
        current.newPath ?? current.oldPath ?? current.displayPath;
      continue;
    }

    if (rawLine.startsWith("+++ ")) {
      const current = ensureFile();
      current.newPath = normalizeDiffPath(rawLine.slice(4));
      current.displayPath =
        current.newPath ?? current.oldPath ?? current.displayPath;
      current.status = inferStatus(
        current.oldPath,
        current.newPath,
        current.binary,
      );
      continue;
    }

    if (
      rawLine.startsWith("Binary files ") ||
      rawLine.startsWith("GIT binary patch")
    ) {
      const current = ensureFile();
      current.binary = true;
      current.status = "binary";
      continue;
    }

    if (rawLine.startsWith("rename from ")) {
      const current = ensureFile();
      current.oldPath = rawLine.slice("rename from ".length);
      current.status = "renamed";
      continue;
    }

    if (rawLine.startsWith("rename to ")) {
      const current = ensureFile();
      current.newPath = rawLine.slice("rename to ".length);
      current.displayPath = `${current.oldPath ?? "unknown"} → ${current.newPath}`;
      current.status = "renamed";
      continue;
    }

    const hunkMatch = rawLine.match(hunkPattern);
    if (hunkMatch !== null) {
      const current = ensureFile();
      const oldStart = Number.parseInt(hunkMatch[1] ?? "0", 10);
      const oldLines = Number.parseInt(hunkMatch[2] ?? "1", 10);
      const newStart = Number.parseInt(hunkMatch[3] ?? "0", 10);
      const newLines = Number.parseInt(hunkMatch[4] ?? "1", 10);
      oldLine = oldStart;
      newLine = newStart;
      hunk = {
        id: `${current.id}:hunk-${current.hunks.length}`,
        header: rawLine,
        oldStart,
        oldLines,
        newStart,
        newLines,
        lines: [],
      };
      current.hunks.push(hunk);
      continue;
    }

    if (hunk !== null) {
      const first = rawLine.at(0) ?? "";
      const kind = lineKind(first);
      const content = kind === "metadata" ? rawLine : rawLine.slice(1);
      const oldLineNumber =
        kind === "addition" || kind === "metadata" ? null : oldLine;
      const newLineNumber =
        kind === "deletion" || kind === "metadata" ? null : newLine;
      const line: DiffLine = {
        id: `${hunk.id}:line-${hunk.lines.length}`,
        kind,
        oldLineNumber,
        newLineNumber,
        content,
        raw: rawLine,
      };
      hunk.lines.push(line);
      if (kind === "context" || kind === "deletion") oldLine += 1;
      if (kind === "context" || kind === "addition") newLine += 1;
    }
  }

  pushFile();
  return files.filter((current) => current.hunks.length > 0 || current.binary);
}

function createFile(
  id: string,
  oldPath: string | null,
  newPath: string | null,
): DiffFile {
  return {
    id,
    oldPath,
    newPath,
    displayPath: newPath ?? oldPath ?? "unknown",
    status: inferStatus(oldPath, newPath, false),
    additions: 0,
    deletions: 0,
    hunks: [],
    binary: false,
    tooLarge: false,
  };
}

function parseGitDiffHeader(line: string): {
  oldPath: string | null;
  newPath: string | null;
} {
  const trimmed = line.slice("diff --git ".length);
  const separator = " b/";
  const separatorIndex = trimmed.indexOf(separator);
  if (!trimmed.startsWith("a/") || separatorIndex === -1) {
    return { oldPath: null, newPath: null };
  }
  return {
    oldPath: trimmed.slice(2, separatorIndex),
    newPath: trimmed.slice(separatorIndex + separator.length),
  };
}

function normalizeDiffPath(value: string): string | null {
  const trimmed = value.trim();
  if (trimmed === "/dev/null") return null;
  if (trimmed.startsWith("a/") || trimmed.startsWith("b/"))
    return trimmed.slice(2);
  return trimmed;
}

function inferStatus(
  oldPath: string | null,
  newPath: string | null,
  binary: boolean,
): DiffFileStatus {
  if (binary) return "binary";
  if (oldPath === null && newPath !== null) return "added";
  if (oldPath !== null && newPath === null) return "deleted";
  if (oldPath !== null && newPath !== null && oldPath !== newPath)
    return "renamed";
  if (oldPath !== null || newPath !== null) return "modified";
  return "unknown";
}

function lineKind(first: string): DiffLineKind {
  switch (first) {
    case "+":
      return "addition";
    case "-":
      return "deletion";
    case " ":
      return "context";
    default:
      return "metadata";
  }
}
