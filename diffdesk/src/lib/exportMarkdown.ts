import type { ReviewComment, SessionFile } from "./types";

export function exportMarkdown(
  session: SessionFile,
  summary: string,
  comments: ReviewComment[],
): string {
  const source = describeSource(session);
  const lines: string[] = [
    "# Diff Review Comments",
    "",
    `Session: \`${session.sessionId}\`  `,
    `Created: \`${session.createdAt}\`  `,
    `Source: \`${source}\``,
    "",
    "## Instructions for AI",
    "",
    "You are receiving human review comments on a code diff. Apply the requested changes carefully, preserve unrelated behavior, and update tests when a comment identifies a correctness issue.",
    "",
    "## Summary",
    "",
    summary.trim() || "No global summary provided.",
    "",
    "## Comments",
    "",
  ];

  if (comments.length === 0) {
    lines.push("No inline comments.");
    return lines.join("\n");
  }

  comments.forEach((comment, index) => {
    const lineNumber =
      comment.anchor.newLineNumber ?? comment.anchor.oldLineNumber;
    lines.push(
      `### ${index + 1}. \`${comment.anchor.path}\``,
      "",
      `Comment ID: \`${comment.id}\`  `,
      `Severity: \`${comment.severity}\`  `,
      `Side: \`${comment.anchor.side}\`  `,
    );
    if (lineNumber !== null) {
      lines.push(`Line: \`${lineNumber}\`  `);
    }
    lines.push(
      "",
      comment.body.trim(),
      "",
      "Relevant line:",
      "",
      "```",
      comment.context.line,
      "```",
      "",
      "---",
      "",
    );
  });

  return lines.join("\n");
}

function describeSource(session: SessionFile): string {
  switch (session.source.kind) {
    case "git": {
      const mode = session.source.staged
        ? "staged"
        : session.source.all
          ? "all local changes"
          : "working tree";
      const repoRoot =
        session.source.repoRoot ?? session.source.repo_root ?? "unknown repo";
      return session.source.range === null
        ? `git ${mode} in ${repoRoot}`
        : `git ${session.source.range} in ${repoRoot}`;
    }
    case "patch-file":
      return `patch file ${session.source.path}`;
    case "stdin":
      return "stdin";
    case "raw":
      return session.source.label;
  }
}
