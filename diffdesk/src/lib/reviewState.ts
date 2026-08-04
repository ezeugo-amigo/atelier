import type { DiffFile, DiffSource, SessionFile } from "./types";

export type FileReview = {
  fingerprint: string;
  reviewedAt: string;
};

export type ReviewState = {
  schemaVersion: number;
  sources: Record<string, Record<string, FileReview>>;
};

export function emptyReviewState(): ReviewState {
  return { schemaVersion: 1, sources: {} };
}

export function loadReviewedFiles(
  session: SessionFile,
  files: DiffFile[],
  state: ReviewState,
): Set<string> {
  const sourceReviews = state.sources[reviewSourceKey(session.source)];
  if (sourceReviews === undefined) return new Set();

  return new Set(
    files
      .filter((file) => {
        const review = sourceReviews[reviewFileKey(file)];
        return review?.fingerprint === reviewFileFingerprint(file);
      })
      .map((file) => file.id),
  );
}

export function reviewSourceKey(source: DiffSource): string {
  switch (source.kind) {
    case "git":
      return JSON.stringify([
        source.kind,
        source.repoRoot ?? source.repo_root ?? "",
        source.range,
        source.staged,
        source.all,
      ]);
    case "patch-file":
      return JSON.stringify([source.kind, source.path]);
    case "stdin":
      return JSON.stringify([source.kind]);
    case "raw":
      return JSON.stringify([source.kind, source.label]);
  }
}

export function reviewFileKey(file: DiffFile): string {
  return JSON.stringify([file.oldPath, file.newPath]);
}

export function reviewFileFingerprint(file: DiffFile): string {
  const material = [
    file.oldPath,
    file.newPath,
    file.displayPath,
    file.status,
    file.additions,
    file.deletions,
    file.binary,
    file.tooLarge,
    ...file.hunks.flatMap((hunk) => [
      hunk.header,
      hunk.oldStart,
      hunk.oldLines,
      hunk.newStart,
      hunk.newLines,
      ...hunk.lines.flatMap((line) => [
        line.kind,
        line.oldLineNumber,
        line.newLineNumber,
        line.content,
        line.raw,
      ]),
    ]),
  ]
    .map((value) => String(value))
    .join("\u0000");

  return hashString(material);
}

function hashString(value: string): string {
  let hash = 2166136261;
  for (let index = 0; index < value.length; index += 1) {
    hash ^= value.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return (hash >>> 0).toString(16).padStart(8, "0");
}
