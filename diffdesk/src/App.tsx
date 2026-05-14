import { invoke } from "@tauri-apps/api/core";
import {
  Check,
  Edit3,
  GitBranch,
  MessageSquare,
  Sparkles,
  X,
} from "lucide-react";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { parseUnifiedDiff } from "./lib/parseUnifiedDiff";
import type {
  DiffFile,
  DiffLine,
  LoadSessionResponse,
  OutputFormat,
  ReviewComment,
  SessionFile,
  SubmitPayload,
  SubmitResult,
} from "./lib/types";

type LoadingState =
  | { kind: "loading" }
  | { kind: "ready"; session: SessionFile; rawDiff: string; files: DiffFile[] }
  | { kind: "error"; message: string };

type LineKey = `${string}::${number}::${number}`;

type AppComment = {
  id: string;
  fileId: string;
  filePath: string;
  startKey: LineKey;
  endKey: LineKey;
  body: string;
  author: string;
  createdAt: string;
  updatedAt: string;
};

type ComposerState = {
  fileId: string;
  filePath: string;
  startKey: LineKey;
  endKey: LineKey;
  body: string;
};

type DragState = {
  fileId: string;
  anchor: LineKey;
  current: LineKey;
};

type ParsedKey = {
  fileId: string;
  hunkIndex: number;
  lineIndex: number;
};

type FlatLine = {
  key: LineKey;
  hunkIndex: number;
  lineIndex: number;
  hunkHeader: string;
  line: DiffLine;
};

const outputFormat: OutputFormat = "markdown";

export function App() {
  const [state, setState] = useState<LoadingState>({ kind: "loading" });
  const [selectedFileId, setSelectedFileId] = useState<string | null>(null);
  const [comments, setComments] = useState<AppComment[]>([]);
  const [composer, setComposer] = useState<ComposerState | null>(null);
  const [drag, setDrag] = useState<DragState | null>(null);
  const [submitted, setSubmitted] = useState(false);
  const [status, setStatus] = useState("Loading session…");
  const dragRef = useRef<DragState | null>(null);
  dragRef.current = drag;

  useEffect(() => {
    async function load() {
      try {
        const sessionId = await invoke<string>("current_session_id");
        const response = await invoke<LoadSessionResponse>("load_session", {
          sessionId,
        });
        const files = parseUnifiedDiff(response.rawDiff);
        setState({
          kind: "ready",
          session: response.session,
          rawDiff: response.rawDiff,
          files,
        });
        setComments(commentsFromDrafts(response.drafts?.comments ?? [], files));
        setStatus(
          files.length === 0
            ? "No changed files in this diff"
            : `${files.length} files loaded`,
        );
      } catch (error) {
        setState({ kind: "error", message: stringifyError(error) });
        setStatus("Failed to load session");
      }
    }
    void load();
  }, []);

  useEffect(() => {
    if (state.kind !== "ready") return;
    const timer = window.setTimeout(() => {
      const reviewComments = commentsToReviewComments(comments, state.files);
      void invoke("save_drafts", {
        sessionId: state.session.sessionId,
        summary: "",
        comments: reviewComments,
      }).then(
        () => setStatus(`Draft saved at ${new Date().toLocaleTimeString()}`),
        (error) => setStatus(`Draft save failed: ${stringifyError(error)}`),
      );
    }, 500);
    return () => window.clearTimeout(timer);
  }, [comments, state]);

  useEffect(() => {
    function handleMouseUp() {
      const currentDrag = dragRef.current;
      if (currentDrag === null) return;
      const [startKey, endKey] = normalizeRange(
        currentDrag.anchor,
        currentDrag.current,
      );
      const file =
        state.kind === "ready"
          ? state.files.find((item) => item.id === currentDrag.fileId)
          : null;
      if (file !== undefined && file !== null) {
        setComposer({
          fileId: file.id,
          filePath: file.newPath ?? file.oldPath ?? file.displayPath,
          startKey,
          endKey,
          body: "",
        });
      }
      setDrag(null);
    }

    window.addEventListener("mouseup", handleMouseUp);
    return () => window.removeEventListener("mouseup", handleMouseUp);
  }, [state]);

  const visibleFiles = useMemo(() => {
    if (state.kind !== "ready") return [];
    return selectedFileId === null
      ? state.files
      : state.files.filter((file) => file.id === selectedFileId);
  }, [selectedFileId, state]);

  const commentCounts = useMemo(() => {
    return comments.reduce<Record<string, number>>((accumulator, comment) => {
      accumulator[comment.fileId] = (accumulator[comment.fileId] ?? 0) + 1;
      return accumulator;
    }, {});
  }, [comments]);

  const handleGutterDown = useCallback(
    (key: LineKey, shiftKey: boolean) => {
      if (submitted) return;
      const parsed = parseLineKey(key);
      if (shiftKey && composer !== null && composer.fileId === parsed.fileId) {
        setComposer({ ...composer, endKey: key });
        return;
      }
      setDrag({ fileId: parsed.fileId, anchor: key, current: key });
    },
    [composer, submitted],
  );

  const handleGutterEnter = useCallback((key: LineKey) => {
    const currentDrag = dragRef.current;
    if (currentDrag === null) return;
    if (parseLineKey(key).fileId !== currentDrag.fileId) return;
    setDrag({ ...currentDrag, current: key });
  }, []);

  const submitComposer = useCallback(() => {
    if (composer === null || composer.body.trim() === "") return;
    const now = new Date().toISOString();
    setComments((current) => [
      ...current,
      {
        id: `cmt_${crypto.randomUUID().replaceAll("-", "")}`,
        fileId: composer.fileId,
        filePath: composer.filePath,
        startKey: composer.startKey,
        endKey: composer.endKey,
        body: composer.body.trim(),
        author: "you",
        createdAt: "just now",
        updatedAt: now,
      },
    ]);
    setComposer(null);
  }, [composer]);

  const editComment = useCallback((id: string, body: string) => {
    setComments((current) =>
      current.map((comment) =>
        comment.id === id
          ? {
              ...comment,
              body: body.trim(),
              createdAt: "edited",
              updatedAt: new Date().toISOString(),
            }
          : comment,
      ),
    );
  }, []);

  const deleteComment = useCallback((id: string) => {
    setComments((current) => current.filter((comment) => comment.id !== id));
  }, []);

  const submitReview = useCallback(async () => {
    if (state.kind !== "ready" || comments.length === 0) return;
    setSubmitted(true);
    setComposer(null);
    setStatus(`Sending ${comments.length} notes to agent…`);
    const payload: SubmitPayload = {
      summary: `Please address the ${comments.length} inline review note${comments.length === 1 ? "" : "s"} below.`,
      comments: commentsToReviewComments(comments, state.files),
      format: outputFormat,
    };
    try {
      const result = await invoke<SubmitResult>("submit_review", {
        sessionId: state.session.sessionId,
        payload,
      });
      setStatus(
        result.outputPath === null
          ? "Review submitted"
          : `Review written to ${result.outputPath}`,
      );
    } catch (error) {
      setSubmitted(false);
      setStatus(`Submit failed: ${stringifyError(error)}`);
    }
  }, [comments, state]);

  if (state.kind === "loading") {
    return (
      <div className="desktop-bg">
        <div className="window loading-window">Loading Diffdesk…</div>
      </div>
    );
  }

  if (state.kind === "error") {
    return (
      <div className="desktop-bg">
        <div className="window loading-window">
          <h1>Could not open diff</h1>
          <p>{state.message}</p>
        </div>
      </div>
    );
  }

  const totals = totalStats(state.files);

  return (
    <div className={`desktop-bg${drag !== null ? " is-dragging" : ""}`}>
      <div className="window">
        <TitleBar
          session={state.session}
          noteCount={comments.length}
          submitted={submitted}
          onSend={() => void submitReview()}
        />
        <div className="window__body">
          <Sidebar
            files={state.files}
            noteCounts={commentCounts}
            selectedFileId={selectedFileId}
            session={state.session}
            totals={totals}
            onSelect={setSelectedFileId}
          />
          <main className="diff-pane">
            <div className="diff-pane__header">
              <div>
                <div className="type-page-title">Review changes</div>
                <div className="type-support diff-pane__subtitle">
                  Click a line number to add a note. Drag across line numbers to
                  span a range. Notes are sent to the agent in one batch.
                </div>
              </div>
              <div className="diff-pane__legend type-mono">
                <span>
                  <span className="legend-dot legend-dot--add" /> added
                </span>
                <span>
                  <span className="legend-dot legend-dot--del" /> removed
                </span>
                <span>
                  <span className="legend-dot legend-dot--note" /> your note
                </span>
              </div>
            </div>

            {visibleFiles.length === 0 ? (
              <EmptyDiff />
            ) : (
              visibleFiles.map((file) => (
                <FileDiff
                  key={file.id}
                  comments={comments}
                  composer={composer}
                  draggingRange={drag}
                  file={file}
                  locked={submitted}
                  onCancelComposer={() => setComposer(null)}
                  onDeleteComment={deleteComment}
                  onEditComment={editComment}
                  onGutterDown={handleGutterDown}
                  onGutterEnter={handleGutterEnter}
                  onSubmitComposer={submitComposer}
                  onUpdateComposerBody={(body) =>
                    setComposer((current) =>
                      current === null ? null : { ...current, body },
                    )
                  }
                />
              ))
            )}

            {submitted ? (
              <div className="agent-receipt">
                <div className="agent-receipt__head">
                  <Sparkles size={15} />
                  <span className="type-card-title">
                    {comments.length} note{comments.length === 1 ? "" : "s"}{" "}
                    sent to agent
                  </span>
                </div>
                <div className="type-body agent-receipt__body">
                  The agent is working on a revised patch. Your notes are locked
                  while it runs.
                </div>
              </div>
            ) : null}

            <div className="diff-pane__footer">
              <div className="type-mono">{status}</div>
              <div className="type-support">
                {state.files.length} files · +{totals.additions} / −
                {totals.deletions}
              </div>
            </div>
          </main>
        </div>
      </div>
    </div>
  );
}

function TitleBar({
  noteCount,
  onSend,
  session,
  submitted,
}: {
  noteCount: number;
  onSend: () => void;
  session: SessionFile;
  submitted: boolean;
}) {
  return (
    <div className="titlebar">
      <div className="titlebar__left">
        <TrafficLights />
      </div>
      <div className="titlebar__center">
        <div className="titlebar__branch">
          <GitBranch size={12} />
          <span className="branch-name">{sourceHead(session)}</span>
          <span className="branch-sep">←</span>
          <span className="branch-base">{sourceBase(session)}</span>
          <span className="branch-dot">·</span>
          <span className="branch-sha">{session.sessionId.slice(4, 11)}</span>
        </div>
      </div>
      <div className="titlebar__right">
        {submitted ? (
          <div className="send-status">
            <span className="send-status__dot" />
            <span>
              Agent running · {noteCount} note{noteCount === 1 ? "" : "s"} sent
            </span>
          </div>
        ) : (
          <button
            className={`btn-send${noteCount === 0 ? " is-disabled" : ""}`}
            disabled={noteCount === 0}
            onClick={onSend}
            type="button"
          >
            <Sparkles size={13} />
            <span>
              Send {noteCount > 0 ? `${noteCount} ` : ""}note
              {noteCount === 1 ? "" : "s"} to agent
            </span>
            <span aria-hidden="true">→</span>
          </button>
        )}
      </div>
    </div>
  );
}

function TrafficLights() {
  return (
    <div className="traffic-lights" aria-hidden="true">
      <span className="traffic-dot close" />
      <span className="traffic-dot minimize" />
      <span className="traffic-dot zoom" />
    </div>
  );
}

function Sidebar({
  files,
  noteCounts,
  onSelect,
  selectedFileId,
  session,
  totals,
}: {
  files: DiffFile[];
  noteCounts: Record<string, number>;
  onSelect: (fileId: string | null) => void;
  selectedFileId: string | null;
  session: SessionFile;
  totals: { additions: number; deletions: number };
}) {
  const totalNotes = Object.values(noteCounts).reduce(
    (sum, count) => sum + count,
    0,
  );
  return (
    <aside className="sidebar">
      <div className="task-card">
        <div className="task-card__eyebrow type-mono">
          <Sparkles size={12} /> Agent run
        </div>
        <div className="task-card__title">Review AI-generated patch</div>
        <div className="task-card__meta">
          <span>Diffdesk</span>
          <span className="meta-dot">·</span>
          <span>{sourceLabel(session)}</span>
        </div>
        <div className="task-card__stats">
          <span>
            <b>{files.length}</b> files
          </span>
          <span className="delta-add">+{totals.additions}</span>
          <span className="delta-del">−{totals.deletions}</span>
        </div>
      </div>

      <div className="sidebar__section-header type-mono">Changed files</div>
      <button
        className={`file-row${selectedFileId === null ? " is-active" : ""}`}
        onClick={() => onSelect(null)}
        type="button"
      >
        <span className="file-row__status" />
        <span className="file-row__name-wrap all-changes">
          <span className="file-row__name">All changes</span>
        </span>
        <span className="file-row__meta">{files.length}</span>
      </button>

      {files.map((file) => {
        const path = file.newPath ?? file.oldPath ?? file.displayPath;
        const parts = path.split("/");
        const name = parts.pop() ?? path;
        const dir = parts.join("/");
        const notes = noteCounts[file.id] ?? 0;
        return (
          <button
            className={`file-row${selectedFileId === file.id ? " is-active" : ""}`}
            key={file.id}
            onClick={() => onSelect(file.id)}
            type="button"
          >
            <span className={`file-row__status status-${file.status}`}>
              {statusLetter(file.status)}
            </span>
            <span className="file-row__name-wrap">
              <span className="file-row__name">{name}</span>
              <span className="file-row__dir">{dir}</span>
            </span>
            <span className="file-row__stats">
              <span className="delta-add">+{file.additions}</span>
              <span className="delta-del">−{file.deletions}</span>
              {notes > 0 ? (
                <span
                  className="file-row__comments"
                  title={`${notes} note${notes === 1 ? "" : "s"}`}
                >
                  <MessageSquare size={9} /> {notes}
                </span>
              ) : null}
            </span>
          </button>
        );
      })}

      <div className="sidebar__footer">
        <div className="type-mono">Workspace</div>
        <div className="ws-name">{workspaceLabel(session)}</div>
        {totalNotes > 0 ? (
          <div className="ws-pending type-support">
            {totalNotes} note{totalNotes === 1 ? "" : "s"} queued for agent
          </div>
        ) : null}
      </div>
    </aside>
  );
}

function FileDiff({
  comments,
  composer,
  draggingRange,
  file,
  locked,
  onCancelComposer,
  onDeleteComment,
  onEditComment,
  onGutterDown,
  onGutterEnter,
  onSubmitComposer,
  onUpdateComposerBody,
}: {
  comments: AppComment[];
  composer: ComposerState | null;
  draggingRange: DragState | null;
  file: DiffFile;
  locked: boolean;
  onCancelComposer: () => void;
  onDeleteComment: (id: string) => void;
  onEditComment: (id: string, body: string) => void;
  onGutterDown: (key: LineKey, shiftKey: boolean) => void;
  onGutterEnter: (key: LineKey) => void;
  onSubmitComposer: () => void;
  onUpdateComposerBody: (body: string) => void;
}) {
  const flat = useMemo(() => flattenFile(file), [file]);
  const dragKeys = useMemo(() => {
    if (draggingRange === null || draggingRange.fileId !== file.id)
      return new Set<LineKey>();
    return keysBetween(flat, draggingRange.anchor, draggingRange.current);
  }, [draggingRange, file.id, flat]);

  const composerKeys = useMemo(() => {
    if (composer === null || composer.fileId !== file.id) return null;
    const [start, end] = normalizeRange(composer.startKey, composer.endKey);
    return { start, end, keys: keysBetween(flat, start, end) };
  }, [composer, file.id, flat]);

  const commentRangeKeys = useMemo(() => {
    const keys = new Set<LineKey>();
    for (const comment of comments.filter((item) => item.fileId === file.id)) {
      for (const key of keysBetween(flat, comment.startKey, comment.endKey))
        keys.add(key);
    }
    return keys;
  }, [comments, file.id, flat]);

  const commentsByEndKey = useMemo(() => {
    return comments
      .filter((comment) => comment.fileId === file.id)
      .reduce<Record<string, AppComment[]>>((accumulator, comment) => {
        const [, end] = normalizeRange(comment.startKey, comment.endKey);
        accumulator[end] = [...(accumulator[end] ?? []), comment];
        return accumulator;
      }, {});
  }, [comments, file.id]);

  return (
    <section className="file">
      <header className="file__head">
        <span className="file__path">
          <span className="file__path-dir">{filePathDir(file)}/</span>
          <span className="file__path-name">{filePathName(file)}</span>
        </span>
        <span className="file__head-stats">
          <span className={`type-badge status-badge status-${file.status}`}>
            {file.status}
          </span>
          <span className="type-mono delta-add">+{file.additions}</span>
          <span className="type-mono delta-del">−{file.deletions}</span>
        </span>
      </header>

      <div className="diff">
        {file.hunks.map((hunk, hunkIndex) => (
          <div className="hunk" key={hunk.id}>
            <div className="hunk-header">
              <span className="type-mono">{hunk.header}</span>
            </div>
            {hunk.lines.map((line, lineIndex) => {
              const key = lineKey(file.id, hunkIndex, lineIndex);
              const commentThread = commentsByEndKey[key] ?? [];
              const rangeSize =
                composerKeys === null ? 1 : composerKeys.keys.size || 1;
              return (
                <div className="line-block" key={line.id}>
                  <DiffLineView
                    hasComment={commentRangeKeys.has(key)}
                    isInRange={
                      dragKeys.has(key) || composerKeys?.keys.has(key) === true
                    }
                    line={line}
                    lineKeyValue={key}
                    onGutterDown={onGutterDown}
                    onGutterEnter={onGutterEnter}
                  />
                  {commentThread.length > 0 ? (
                    <div className="thread">
                      {commentThread.map((comment) => (
                        <CommentBubble
                          comment={comment}
                          key={comment.id}
                          locked={locked}
                          onDelete={() => onDeleteComment(comment.id)}
                          onEdit={(body) => onEditComment(comment.id, body)}
                        />
                      ))}
                    </div>
                  ) : null}
                  {composerKeys !== null && key === composerKeys.end ? (
                    <div className="thread thread--composer">
                      <Composer
                        body={composer?.body ?? ""}
                        rangeSize={rangeSize}
                        onBodyChange={onUpdateComposerBody}
                        onCancel={onCancelComposer}
                        onSubmit={onSubmitComposer}
                      />
                    </div>
                  ) : null}
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </section>
  );
}

function DiffLineView({
  hasComment,
  isInRange,
  line,
  lineKeyValue,
  onGutterDown,
  onGutterEnter,
}: {
  hasComment: boolean;
  isInRange: boolean;
  line: DiffLine;
  lineKeyValue: LineKey;
  onGutterDown: (key: LineKey, shiftKey: boolean) => void;
  onGutterEnter: (key: LineKey) => void;
}) {
  const marker =
    line.kind === "addition" ? "+" : line.kind === "deletion" ? "−" : " ";
  return (
    <div
      className={`dl dl--${line.kind}${isInRange ? " is-in-range" : ""}${hasComment ? " has-comment" : ""}`}
      onMouseEnter={() => onGutterEnter(lineKeyValue)}
    >
      <button
        className="dl__gutter dl__gutter--old"
        onMouseDown={(event) =>
          handleLineMouseDown(event, lineKeyValue, onGutterDown)
        }
        type="button"
      >
        <span className="dl__num">{line.oldLineNumber ?? ""}</span>
        <span className="dl__add">+</span>
      </button>
      <button
        className="dl__gutter dl__gutter--new"
        onMouseDown={(event) =>
          handleLineMouseDown(event, lineKeyValue, onGutterDown)
        }
        type="button"
      >
        <span className="dl__num">{line.newLineNumber ?? ""}</span>
      </button>
      <span className="dl__marker">{marker}</span>
      <pre className="dl__code">{line.content || "\u00A0"}</pre>
    </div>
  );
}

function Composer({
  body,
  onBodyChange,
  onCancel,
  onSubmit,
  rangeSize,
}: {
  body: string;
  onBodyChange: (body: string) => void;
  onCancel: () => void;
  onSubmit: () => void;
  rangeSize: number;
}) {
  const textAreaRef = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => textAreaRef.current?.focus(), []);
  return (
    <div className="composer">
      <div className="composer__head">
        <span className="type-mono">
          {rangeSize > 1 ? `Note on ${rangeSize} lines` : "Note on this line"}
        </span>
        <span className="type-mono composer__hint">
          ⌘↵ to add · Esc to cancel
        </span>
      </div>
      <textarea
        className="comment__textarea"
        onChange={(event) => onBodyChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
          }
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
            event.preventDefault();
            onSubmit();
          }
        }}
        placeholder="What should the agent change?"
        ref={textAreaRef}
        value={body}
      />
      <div className="comment__actions">
        <button className="btn btn--ghost" onClick={onCancel} type="button">
          Cancel
        </button>
        <button
          className={`btn btn--primary${body.trim() === "" ? " is-disabled" : ""}`}
          disabled={body.trim() === ""}
          onClick={onSubmit}
          type="button"
        >
          Add note
        </button>
      </div>
    </div>
  );
}

function CommentBubble({
  comment,
  locked,
  onDelete,
  onEdit,
}: {
  comment: AppComment;
  locked: boolean;
  onDelete: () => void;
  onEdit: (body: string) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(comment.body);
  useEffect(() => setDraft(comment.body), [comment.body]);

  if (editing) {
    return (
      <div className="comment comment--editing">
        <div className="comment__head">
          <span className="comment__author">{comment.author}</span>
          <span className="type-mono comment__when">Editing</span>
        </div>
        <textarea
          className="comment__textarea"
          onChange={(event) => setDraft(event.target.value)}
          value={draft}
        />
        <div className="comment__actions">
          <button
            className="btn btn--ghost"
            onClick={() => setEditing(false)}
            type="button"
          >
            Cancel
          </button>
          <button
            className={`btn btn--primary${draft.trim() === "" ? " is-disabled" : ""}`}
            disabled={draft.trim() === ""}
            onClick={() => {
              onEdit(draft);
              setEditing(false);
            }}
            type="button"
          >
            Save
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className={`comment${locked ? " comment--locked" : ""}`}>
      <div className="comment__head">
        <span className="comment__author">{comment.author}</span>
        <span className="type-mono comment__when">{comment.createdAt}</span>
        {locked ? <span className="type-badge is-product">Queued</span> : null}
        {!locked ? (
          <div className="comment__row-actions">
            <button
              className="icon-btn"
              onClick={() => setEditing(true)}
              title="Edit"
              type="button"
            >
              <Edit3 size={12} />
            </button>
            <button
              className="icon-btn"
              onClick={onDelete}
              title="Delete"
              type="button"
            >
              <X size={12} />
            </button>
          </div>
        ) : null}
      </div>
      <div className="comment__body">{comment.body}</div>
    </div>
  );
}

function EmptyDiff() {
  return (
    <div className="empty-diff">
      <Check size={28} />
      <h1>No changes to review</h1>
      <p>
        This diff is empty. Try staged changes, all local changes, a range, a
        patch file, or stdin.
      </p>
    </div>
  );
}

function commentsFromDrafts(
  reviewComments: ReviewComment[],
  files: DiffFile[],
): AppComment[] {
  return reviewComments.flatMap((comment) => {
    const file =
      files.find((item) => item.id === comment.anchor.fileId) ??
      files.find(
        (item) =>
          (item.newPath ?? item.oldPath ?? item.displayPath) ===
          comment.anchor.path,
      );
    if (file === undefined) return [];
    const key = findLineKeyForReviewComment(file, comment);
    if (key === null) return [];
    return [
      {
        id: comment.id,
        fileId: file.id,
        filePath: comment.anchor.path,
        startKey: key,
        endKey: key,
        body: comment.body,
        author: "you",
        createdAt: "draft",
        updatedAt: comment.updatedAt,
      },
    ];
  });
}

function commentsToReviewComments(
  comments: AppComment[],
  files: DiffFile[],
): ReviewComment[] {
  return comments.map((comment) => {
    const file = files.find((item) => item.id === comment.fileId);
    const start =
      file === undefined ? null : lineForKey(file, comment.startKey);
    const end = file === undefined ? null : lineForKey(file, comment.endKey);
    const anchorLine = end ?? start;
    const hunkHeader =
      file === undefined ? "" : hunkHeaderForKey(file, comment.endKey);
    const path = file?.newPath ?? file?.oldPath ?? comment.filePath;
    const side = anchorLine?.kind === "deletion" ? "old" : "new";
    return {
      id: comment.id,
      createdAt: comment.updatedAt,
      updatedAt: new Date().toISOString(),
      severity: "suggestion",
      body: comment.body,
      anchor: {
        kind: comment.startKey === comment.endKey ? "line" : "range",
        fileId: comment.fileId,
        path,
        side,
        oldLineNumber: anchorLine?.oldLineNumber ?? null,
        newLineNumber: anchorLine?.newLineNumber ?? null,
        lineId: anchorLine?.id ?? comment.endKey,
      },
      context: {
        line: rangeContext(file, comment.startKey, comment.endKey),
        hunkHeader,
      },
    };
  });
}

function findLineKeyForReviewComment(
  file: DiffFile,
  comment: ReviewComment,
): LineKey | null {
  const flat = flattenFile(file);
  const match = flat.find(({ line }) => {
    if (comment.anchor.lineId === line.id) return true;
    if (comment.anchor.side === "old")
      return line.oldLineNumber === comment.anchor.oldLineNumber;
    return line.newLineNumber === comment.anchor.newLineNumber;
  });
  return match?.key ?? null;
}

function flattenFile(file: DiffFile): FlatLine[] {
  return file.hunks.flatMap((hunk, hunkIndex) =>
    hunk.lines.map((line, lineIndex) => ({
      key: lineKey(file.id, hunkIndex, lineIndex),
      hunkIndex,
      lineIndex,
      hunkHeader: hunk.header,
      line,
    })),
  );
}

function keysBetween(
  flat: FlatLine[],
  keyA: LineKey,
  keyB: LineKey,
): Set<LineKey> {
  const [startKey, endKey] = normalizeRange(keyA, keyB);
  const start = flat.findIndex((item) => item.key === startKey);
  const end = flat.findIndex((item) => item.key === endKey);
  if (start === -1 || end === -1) return new Set();
  return new Set(flat.slice(start, end + 1).map((item) => item.key));
}

function normalizeRange(keyA: LineKey, keyB: LineKey): [LineKey, LineKey] {
  return compareLineKeys(keyA, keyB) <= 0 ? [keyA, keyB] : [keyB, keyA];
}

function compareLineKeys(keyA: LineKey, keyB: LineKey): number {
  const parsedA = parseLineKey(keyA);
  const parsedB = parseLineKey(keyB);
  if (parsedA.hunkIndex !== parsedB.hunkIndex)
    return parsedA.hunkIndex - parsedB.hunkIndex;
  return parsedA.lineIndex - parsedB.lineIndex;
}

function parseLineKey(key: LineKey): ParsedKey {
  const [fileId, hunkIndex, lineIndex] = key.split("::");
  return {
    fileId: fileId ?? "",
    hunkIndex: Number.parseInt(hunkIndex ?? "0", 10),
    lineIndex: Number.parseInt(lineIndex ?? "0", 10),
  };
}

function lineKey(
  fileId: string,
  hunkIndex: number,
  lineIndex: number,
): LineKey {
  return `${fileId}::${hunkIndex}::${lineIndex}`;
}

function lineForKey(file: DiffFile, key: LineKey): DiffLine | null {
  const parsed = parseLineKey(key);
  return file.hunks.at(parsed.hunkIndex)?.lines.at(parsed.lineIndex) ?? null;
}

function hunkHeaderForKey(file: DiffFile, key: LineKey): string {
  const parsed = parseLineKey(key);
  return file.hunks.at(parsed.hunkIndex)?.header ?? "";
}

function rangeContext(
  file: DiffFile | undefined,
  startKey: LineKey,
  endKey: LineKey,
): string {
  if (file === undefined) return "";
  const keys = keysBetween(flattenFile(file), startKey, endKey);
  return flattenFile(file)
    .filter((item) => keys.has(item.key))
    .map(({ line }) => line.content)
    .join("\n");
}

function handleLineMouseDown(
  event: React.MouseEvent<HTMLButtonElement>,
  key: LineKey,
  onGutterDown: (key: LineKey, shiftKey: boolean) => void,
) {
  event.preventDefault();
  onGutterDown(key, event.shiftKey);
}

function filePath(file: DiffFile): string {
  return file.newPath ?? file.oldPath ?? file.displayPath;
}

function filePathName(file: DiffFile): string {
  const parts = filePath(file).split("/");
  return parts.at(-1) ?? file.displayPath;
}

function filePathDir(file: DiffFile): string {
  const parts = filePath(file).split("/");
  return parts.slice(0, -1).join("/");
}

function totalStats(files: DiffFile[]): {
  additions: number;
  deletions: number;
} {
  return files.reduce(
    (totals, file) => ({
      additions: totals.additions + file.additions,
      deletions: totals.deletions + file.deletions,
    }),
    { additions: 0, deletions: 0 },
  );
}

function statusLetter(status: DiffFile["status"]): string {
  switch (status) {
    case "added":
      return "A";
    case "deleted":
      return "D";
    case "renamed":
      return "R";
    case "modified":
      return "M";
    case "binary":
      return "B";
    case "unknown":
      return "?";
  }
}

function workspaceLabel(session: SessionFile): string {
  if (session.source.kind !== "git") return sourceLabel(session);
  const repoRoot =
    session.source.repoRoot ?? session.source.repo_root ?? "unknown repo";
  const name = repoRoot.split("/").filter(Boolean).at(-1) ?? repoRoot;
  return `${name} · ${repoRoot}`;
}

function sourceLabel(session: SessionFile): string {
  switch (session.source.kind) {
    case "git":
      if (session.source.range !== null) return session.source.range;
      if (session.source.staged) return "staged changes";
      if (session.source.all) return "all local changes";
      return "working tree";
    case "patch-file":
      return "patch file";
    case "stdin":
      return "stdin";
    case "raw":
      return session.source.label;
  }
}

function sourceHead(session: SessionFile): string {
  if (session.source.kind === "git" && session.source.range !== null)
    return (
      session.source.range.split("...").at(-1)?.split("..").at(-1) ?? "HEAD"
    );
  if (session.source.kind === "git")
    return session.source.staged
      ? "staged"
      : session.source.all
        ? "local"
        : "working tree";
  return session.source.kind;
}

function sourceBase(session: SessionFile): string {
  if (session.source.kind === "git" && session.source.range !== null)
    return session.source.range.split("...").at(0)?.split("..").at(0) ?? "base";
  return "HEAD";
}

function stringifyError(error: unknown): string {
  if (error instanceof Error) return error.message;
  if (typeof error === "string") return error;
  return JSON.stringify(error);
}
