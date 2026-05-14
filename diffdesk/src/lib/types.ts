export type OutputFormat = "markdown" | "json";

export type SessionFile = {
  schemaVersion: string;
  sessionId: string;
  createdAt: string;
  sessionDir: string;
  inputDiffPath: string;
  source: DiffSource;
  options: SessionOptions;
};

export type DiffSource =
  | {
      kind: "git";
      repoRoot?: string;
      workingDirectory?: string;
      repo_root?: string;
      working_directory?: string;
      range: string | null;
      staged: boolean;
      all: boolean;
    }
  | { kind: "patch-file"; path: string }
  | { kind: "stdin" }
  | { kind: "raw"; label: string };

export type SessionOptions = {
  wait: boolean;
  outputPath: string | null;
  outputFormat: OutputFormat;
  copyToClipboard: boolean;
  aiCommand: string | null;
};

export type DraftFile = {
  schemaVersion: string;
  sessionId: string;
  savedAt: string;
  summary: string;
  comments: ReviewComment[];
};

export type LoadSessionResponse = {
  session: SessionFile;
  rawDiff: string;
  drafts: DraftFile | null;
};

export type CommentSeverity =
  | "note"
  | "question"
  | "suggestion"
  | "issue"
  | "blocking";

export type CommentAnchor = {
  kind: string;
  fileId: string;
  path: string;
  side: "old" | "new";
  oldLineNumber: number | null;
  newLineNumber: number | null;
  lineId: string;
};

export type CommentContext = {
  line: string;
  hunkHeader: string;
};

export type ReviewComment = {
  id: string;
  createdAt: string;
  updatedAt: string;
  severity: CommentSeverity;
  body: string;
  anchor: CommentAnchor;
  context: CommentContext;
};

export type SubmitPayload = {
  summary: string;
  comments: ReviewComment[];
  format: OutputFormat;
};

export type SubmitResult = {
  schemaVersion: string;
  sessionId: string;
  status: string;
  submittedAt: string;
  outputPath: string | null;
  resultPath: string;
};

export type DiffFileStatus =
  | "added"
  | "modified"
  | "deleted"
  | "renamed"
  | "binary"
  | "unknown";

export type DiffFile = {
  id: string;
  oldPath: string | null;
  newPath: string | null;
  displayPath: string;
  status: DiffFileStatus;
  additions: number;
  deletions: number;
  hunks: DiffHunk[];
  binary: boolean;
  tooLarge: boolean;
};

export type DiffHunk = {
  id: string;
  header: string;
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  lines: DiffLine[];
};

export type DiffLineKind = "context" | "addition" | "deletion" | "metadata";

export type DiffLine = {
  id: string;
  kind: DiffLineKind;
  oldLineNumber: number | null;
  newLineNumber: number | null;
  content: string;
  raw: string;
};
