import type { DiffLineAnnotation, SelectedLineRange } from "@pierre/diffs";
import { PatchDiff } from "@pierre/diffs/react";
import { invoke } from "@tauri-apps/api/core";
import {
  Check,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Edit3,
  Folder,
  FolderOpen,
  GitBranch,
  MessageSquare,
  Search,
  Sparkles,
  X,
} from "lucide-react";
import {
  type RefObject,
  useCallback,
  useEffect,
  useMemo,
  useReducer,
  useRef,
  useState,
} from "react";
import {
  clampIndex,
  findQuery,
  findReducer,
  initialFindState,
} from "./lib/findState";
import { parseUnifiedDiff } from "./lib/parseUnifiedDiff";
import {
  emptyReviewState,
  loadReviewedFiles,
  reviewFileFingerprint,
  reviewFileKey,
  reviewSourceKey,
  type ReviewState,
} from "./lib/reviewState";
import {
  applyFindHighlights,
  buildSearchMatches,
  filePath,
  findHighlightCSS,
  scrollMatchIntoView,
} from "./lib/search";
import type {
  CommentSeverity,
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
  severity: CommentSeverity;
  author: string;
  createdAt: string;
  updatedAt: string;
};

type ComposerState = {
  fileId: string;
  filePath: string;
  startKey: LineKey;
  endKey: LineKey;
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

type DiffdeskAnnotation =
  | { kind: "comment"; comment: AppComment }
  | { kind: "composer"; rangeSize: number };

const severityOptions: Array<{ value: CommentSeverity; label: string }> = [
  { value: "note", label: "Note" },
  { value: "question", label: "Question" },
  { value: "suggestion", label: "Suggestion" },
  { value: "issue", label: "Issue" },
  { value: "blocking", label: "Blocking" },
];

type SidebarFolderGroup = {
  id: string;
  label: string;
  files: DiffFile[];
  additions: number;
  deletions: number;
};

const outputFormat: OutputFormat = "markdown";

export function App() {
  const [state, setState] = useState<LoadingState>({ kind: "loading" });
  const [selectedFileId, setSelectedFileId] = useState<string | null>(null);
  const [comments, setComments] = useState<AppComment[]>([]);
  const [summary, setSummary] = useState("");
  const [composer, setComposer] = useState<ComposerState | null>(null);
  const [submitted, setSubmitted] = useState(false);
  const [status, setStatus] = useState("Loading session…");
  const [viewedFiles, setViewedFiles] = useState<Set<string>>(new Set());
  const [collapsedFiles, setCollapsedFiles] = useState<Set<string>>(new Set());
  const [collapsedFolders, setCollapsedFolders] = useState<Set<string>>(
    new Set(),
  );
  const [find, dispatchFind] = useReducer(findReducer, initialFindState);
  const diffPaneRef = useRef<HTMLElement | null>(null);
  const searchInputRef = useRef<HTMLInputElement | null>(null);

  useEffect(() => {
    async function load() {
      try {
        const sessionId = await invoke<string>("current_session_id");
        const response = await invoke<LoadSessionResponse>("load_session", {
          sessionId,
        });
        const reviewState = await invoke<ReviewState>("load_review_state").catch(
          () => emptyReviewState(),
        );
        const files = parseUnifiedDiff(response.rawDiff);
        const reviewedFiles = loadReviewedFiles(
          response.session,
          files,
          reviewState,
        );
        setState({
          kind: "ready",
          session: response.session,
          rawDiff: response.rawDiff,
          files,
        });
        setCollapsedFiles(
          new Set(files.filter((file) => file.tooLarge).map((file) => file.id)),
        );
        setSummary(response.drafts?.summary ?? "");
        setComments(commentsFromDrafts(response.drafts?.comments ?? [], files));
        setViewedFiles(reviewedFiles);
        setCollapsedFiles(new Set(reviewedFiles));
        setStatus(
          files.length === 0
            ? "No changed files in this diff"
            : reviewedFiles.size === 0
              ? `${files.length} files loaded`
              : `${files.length} files loaded · ${reviewedFiles.size} already reviewed`,
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
        summary,
        comments: reviewComments,
      }).then(
        () => setStatus(`Draft saved at ${new Date().toLocaleTimeString()}`),
        (error) => setStatus(`Draft save failed: ${stringifyError(error)}`),
      );
    }, 500);
    return () => window.clearTimeout(timer);
  }, [comments, state, summary]);

  const visibleFiles = useMemo(() => {
    if (state.kind !== "ready") return [];
    return selectedFileId === null
      ? state.files
      : state.files.filter((file) => file.id === selectedFileId);
  }, [selectedFileId, state]);

  const query = findQuery(find);
  const searchMatches = useMemo(
    () => buildSearchMatches(visibleFiles, query),
    [query, visibleFiles],
  );
  const activeSearchIndex =
    find.kind === "open" ? clampIndex(find.matchIndex, searchMatches.length) : 0;
  const activeSearchMatch =
    find.kind === "open" ? (searchMatches[activeSearchIndex] ?? null) : null;

  // Editing a file (or narrowing to one) can shrink the result set under a
  // cursor that was valid a render ago.
  useEffect(() => {
    dispatchFind({ type: "clampIndex", matchCount: searchMatches.length });
  }, [searchMatches.length]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if (
        event.key.toLowerCase() === "f" &&
        (event.metaKey || event.ctrlKey) &&
        !event.altKey
      ) {
        // Writing a note outranks searching: stealing focus mid-sentence loses
        // the user's place. The find bar's own input is not a note editor, so
        // ⌘F inside it still reselects the query.
        if (isNoteEditor(event.target)) return;
        event.preventDefault();
        dispatchFind({ type: "open" });
        return;
      }

      if (event.key === "Escape" && !isNoteEditor(event.target)) {
        dispatchFind({ type: "close" });
      }
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const focusToken = find.kind === "open" ? find.focusToken : null;
  useEffect(() => {
    if (focusToken === null) return;
    window.requestAnimationFrame(() => {
      searchInputRef.current?.focus();
      searchInputRef.current?.select();
    });
  }, [focusToken]);

  // @pierre/diffs replaces its shadow DOM's innerHTML on every render, which
  // invalidates the DOM ranges the highlights are built from. Repainting reads
  // the current search through a ref so the callback identity can stay stable:
  // it is handed to PatchDiff as an option, and a changing option forces a full
  // re-render of the diff.
  const findRef = useRef({ activeIndex: 0, matches: searchMatches, query });
  findRef.current = { activeIndex: activeSearchIndex, matches: searchMatches, query };

  const repaintHighlights = useCallback(() => {
    const { activeIndex, matches, query: current } = findRef.current;
    applyFindHighlights({
      activeIndex,
      matches,
      pane: diffPaneRef.current,
      query: current,
    });
  }, []);

  const onFileRendered = useCallback(() => {
    // onPostRender fires inside pierre's own layout effect, before the new nodes
    // are laid out. Defer so ranges resolve against the settled tree.
    window.requestAnimationFrame(repaintHighlights);
  }, [repaintHighlights]);

  // Expanding a collapsed file, painting, and scrolling all wait for the diff to
  // re-render for this query, hence the double rAF.
  useEffect(() => {
    if (activeSearchMatch === null) {
      repaintHighlights();
      return;
    }

    setCollapsedFiles((current) => {
      if (!current.has(activeSearchMatch.fileId)) return current;
      const next = new Set(current);
      next.delete(activeSearchMatch.fileId);
      return next;
    });

    let nextFrame = 0;
    const frame = window.requestAnimationFrame(() => {
      nextFrame = window.requestAnimationFrame(() => {
        repaintHighlights();
        scrollMatchIntoView(diffPaneRef.current, activeSearchMatch);
      });
    });
    return () => {
      window.cancelAnimationFrame(frame);
      window.cancelAnimationFrame(nextFrame);
    };
  }, [
    activeSearchIndex,
    activeSearchMatch,
    query,
    repaintHighlights,
    searchMatches,
  ]);

  const stepSearch = useCallback(
    (direction: 1 | -1) => {
      dispatchFind({ type: "step", direction, matchCount: searchMatches.length });
    },
    [searchMatches.length],
  );

  const commentCounts = useMemo(() => {
    return comments.reduce<Record<string, number>>((accumulator, comment) => {
      accumulator[comment.fileId] = (accumulator[comment.fileId] ?? 0) + 1;
      return accumulator;
    }, {});
  }, [comments]);

  // Marking a file "viewed" also collapses it (and un-viewing re-expands it),
  // mirroring the GitHub review flow.
  const toggleViewed = useCallback(
    (fileId: string) => {
      const willView = !viewedFiles.has(fileId);
      setViewedFiles((current) => {
        const next = new Set(current);
        if (willView) next.add(fileId);
        else next.delete(fileId);
        return next;
      });
      setCollapsedFiles((current) => {
        const next = new Set(current);
        if (willView) next.add(fileId);
        else next.delete(fileId);
        return next;
      });
      if (state.kind === "ready") {
        const file = state.files.find((item) => item.id === fileId);
        if (file !== undefined) {
          void invoke("save_file_review", {
            sourceKey: reviewSourceKey(state.session.source),
            fileKey: reviewFileKey(file),
            fingerprint: reviewFileFingerprint(file),
            reviewed: willView,
          }).then(
            () =>
              setStatus(
                willView
                  ? "Viewed state saved"
                  : "Viewed state cleared",
              ),
            (error) =>
              setStatus(`Review state save failed: ${stringifyError(error)}`),
          );
        }
      }
    },
    [state, viewedFiles],
  );

  const toggleCollapsed = useCallback((fileId: string) => {
    setCollapsedFiles((current) => {
      const next = new Set(current);
      if (next.has(fileId)) next.delete(fileId);
      else next.add(fileId);
      return next;
    });
  }, []);

  const toggleFolderCollapsed = useCallback((folderId: string) => {
    setCollapsedFolders((current) => {
      const next = new Set(current);
      if (next.has(folderId)) next.delete(folderId);
      else next.add(folderId);
      return next;
    });
  }, []);

  const submitComposer = useCallback(
    (body: string, severity: CommentSeverity) => {
      if (composer === null || body.trim() === "") return;
      const now = new Date().toISOString();
      setComments((current) => [
        ...current,
        {
          id: `cmt_${crypto.randomUUID().replaceAll("-", "")}`,
          fileId: composer.fileId,
          filePath: composer.filePath,
          startKey: composer.startKey,
          endKey: composer.endKey,
          body: body.trim(),
          severity,
          author: "you",
          createdAt: "just now",
          updatedAt: now,
        },
      ]);
      setComposer(null);
    },
    [composer],
  );

  const editComment = useCallback(
    (id: string, body: string, severity: CommentSeverity) => {
      setComments((current) =>
        current.map((comment) =>
          comment.id === id
            ? {
                ...comment,
                body: body.trim(),
                severity,
                createdAt: "edited",
                updatedAt: new Date().toISOString(),
              }
            : comment,
        ),
      );
    },
    [],
  );

  const deleteComment = useCallback((id: string) => {
    setComments((current) => current.filter((comment) => comment.id !== id));
  }, []);

  const clearComments = useCallback(() => {
    if (comments.length === 0 || submitted) return;
    const confirmed = window.confirm(
      `Clear all ${comments.length} queued note${comments.length === 1 ? "" : "s"}?`,
    );
    if (!confirmed) return;
    setComments([]);
    setComposer(null);
    setStatus("All queued notes cleared");
  }, [comments.length, submitted]);

  const submitReview = useCallback(async () => {
    if (
      state.kind !== "ready" ||
      (comments.length === 0 && summary.trim() === "")
    )
      return;
    setSubmitted(true);
    setComposer(null);
    setStatus(
      comments.length > 0
        ? `Sending ${comments.length} notes to agent…`
        : "Sending review to agent…",
    );
    const payload: SubmitPayload = {
      summary:
        summary.trim() ||
        `Please address the ${comments.length} inline review note${comments.length === 1 ? "" : "s"} below.`,
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
  }, [comments, state, summary]);

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
    <div className="desktop-bg">
      <div className="window">
        <TitleBar
          canSend={comments.length > 0 || summary.trim() !== ""}
          session={state.session}
          noteCount={comments.length}
          submitted={submitted}
          onClear={clearComments}
          onSend={() => void submitReview()}
        />
        <div className="window__body">
          <Sidebar
            collapsedFolders={collapsedFolders}
            files={state.files}
            noteCounts={commentCounts}
            selectedFileId={selectedFileId}
            session={state.session}
            totals={totals}
            viewedFiles={viewedFiles}
            onSelect={setSelectedFileId}
            onToggleFolder={toggleFolderCollapsed}
          />
          <main className="diff-pane" ref={diffPaneRef}>
            <div className="diff-pane__header">
              <div>
                <div className="type-page-title">Review changes</div>
                <div className="type-support diff-pane__subtitle">
                  Click a line number to add a note. Drag across line numbers to
                  span a range. Notes are sent to the agent in one batch.
                </div>
              </div>
              <div className="diff-pane__tools">
                <button
                  className="tool-icon-btn"
                  onClick={() => dispatchFind({ type: "open" })}
                  title="Search diff"
                  type="button"
                >
                  <Search size={14} />
                </button>
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
            </div>

            {find.kind === "open" ? (
              <FindBar
                inputRef={searchInputRef}
                matchCount={searchMatches.length}
                matchIndex={activeSearchIndex}
                onChange={(value) =>
                  dispatchFind({ type: "setQuery", query: value })
                }
                onClose={() => dispatchFind({ type: "close" })}
                onNext={() => stepSearch(1)}
                onPrevious={() => stepSearch(-1)}
                query={find.query}
              />
            ) : null}

            <div className="review-summary">
              <div className="review-summary__head">
                <label className="type-mono" htmlFor="review-summary">
                  Review summary
                </label>
                <span className="type-support">
                  Optional context for the agent
                </span>
              </div>
              <textarea
                className="review-summary__textarea"
                disabled={submitted}
                id="review-summary"
                onChange={(event) => setSummary(event.target.value)}
                placeholder="Describe the overall outcome you want from this review."
                value={summary}
              />
            </div>

            {visibleFiles.length === 0 ? (
              <EmptyDiff />
            ) : (
              visibleFiles.map((file) => (
                <FileDiff
                  key={file.id}
                  collapsed={collapsedFiles.has(file.id)}
                  comments={comments}
                  composer={composer}
                  file={file}
                  locked={submitted}
                  viewed={viewedFiles.has(file.id)}
                  onCancelComposer={() => setComposer(null)}
                  onDeleteComment={deleteComment}
                  onEditComment={editComment}
                  onRendered={onFileRendered}
                  onToggleCollapsed={toggleCollapsed}
                  onToggleViewed={toggleViewed}
                  onSelectRange={(file, startKey, endKey) => {
                    setComposer({
                      fileId: file.id,
                      filePath:
                        file.newPath ?? file.oldPath ?? file.displayPath,
                      startKey,
                      endKey,
                    });
                  }}
                  onSubmitComposer={submitComposer}
                />
              ))
            )}

            {submitted ? (
              <div className="agent-receipt">
                <div className="agent-receipt__head">
                  <Sparkles size={15} />
                  <span className="type-card-title">
                    {comments.length > 0
                      ? `${comments.length} note${comments.length === 1 ? "" : "s"} sent to agent`
                      : "Review sent to agent"}
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
                {viewedFiles.size}/{state.files.length} viewed · +
                {totals.additions} / −{totals.deletions}
              </div>
            </div>
          </main>
        </div>
      </div>
    </div>
  );
}

function TitleBar({
  canSend,
  noteCount,
  onClear,
  onSend,
  session,
  submitted,
}: {
  canSend: boolean;
  noteCount: number;
  onClear: () => void;
  onSend: () => void;
  session: SessionFile;
  submitted: boolean;
}) {
  return (
    <div className="titlebar">
      <div className="titlebar__meta">
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
          <>
            <button
              className={`btn-clear${noteCount === 0 ? " is-disabled" : ""}`}
              disabled={noteCount === 0}
              onClick={onClear}
              type="button"
            >
              Clear all
            </button>
            <button
              className={`btn-send${!canSend ? " is-disabled" : ""}`}
              disabled={!canSend}
              onClick={onSend}
              type="button"
            >
              <Sparkles size={13} />
              <span>
                {noteCount > 0
                  ? `Send ${noteCount} note${noteCount === 1 ? "" : "s"} to agent`
                  : "Send review to agent"}
              </span>
              <span aria-hidden="true">→</span>
            </button>
          </>
        )}
      </div>
    </div>
  );
}

function FindBar({
  inputRef,
  matchCount,
  matchIndex,
  onChange,
  onClose,
  onNext,
  onPrevious,
  query,
}: {
  inputRef: RefObject<HTMLInputElement | null>;
  matchCount: number;
  matchIndex: number;
  onChange: (value: string) => void;
  onClose: () => void;
  onNext: () => void;
  onPrevious: () => void;
  query: string;
}) {
  const hasQuery = query.trim() !== "";
  const counter = hasQuery
    ? matchCount === 0
      ? "0 results"
      : `${matchIndex + 1} of ${matchCount}`
    : "Type to search";

  return (
    <div className="findbar">
      <Search size={14} />
      <input
        aria-label="Search diff"
        className="findbar__input"
        onChange={(event) => onChange(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            if (event.shiftKey) onPrevious();
            else onNext();
          }
          if (event.key === "Escape") {
            event.preventDefault();
            onClose();
          }
        }}
        placeholder="Search diff"
        ref={inputRef}
        type="search"
        value={query}
      />
      <span className="findbar__count type-mono">{counter}</span>
      <div className="findbar__actions">
        <button
          className="tool-icon-btn"
          disabled={matchCount === 0}
          onClick={onPrevious}
          title="Previous result"
          type="button"
        >
          <ChevronUp size={14} />
        </button>
        <button
          className="tool-icon-btn"
          disabled={matchCount === 0}
          onClick={onNext}
          title="Next result"
          type="button"
        >
          <ChevronDown size={14} />
        </button>
        <button
          className="tool-icon-btn"
          onClick={onClose}
          title="Close search"
          type="button"
        >
          <X size={14} />
        </button>
      </div>
    </div>
  );
}

function Sidebar({
  collapsedFolders,
  files,
  noteCounts,
  onSelect,
  onToggleFolder,
  selectedFileId,
  session,
  totals,
  viewedFiles,
}: {
  collapsedFolders: Set<string>;
  files: DiffFile[];
  noteCounts: Record<string, number>;
  onSelect: (fileId: string | null) => void;
  onToggleFolder: (folderId: string) => void;
  selectedFileId: string | null;
  session: SessionFile;
  totals: { additions: number; deletions: number };
  viewedFiles: Set<string>;
}) {
  const fileGroups = useMemo(() => groupFilesByTopLevelFolder(files), [files]);
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

      {fileGroups.map((group) => {
        const collapsed = collapsedFolders.has(group.id);
        const notes = group.files.reduce(
          (sum, file) => sum + (noteCounts[file.id] ?? 0),
          0,
        );
        return (
          <div className="folder-group" key={group.id}>
            <button
              aria-expanded={!collapsed}
              className={`folder-row${collapsed ? " is-collapsed" : ""}`}
              onClick={() => onToggleFolder(group.id)}
              title={`${collapsed ? "Expand" : "Collapse"} ${group.label}`}
              type="button"
            >
              <span className="folder-row__toggle">
                {collapsed ? (
                  <ChevronRight size={13} />
                ) : (
                  <ChevronDown size={13} />
                )}
              </span>
              <span className="folder-row__name-wrap">
                {collapsed ? <Folder size={13} /> : <FolderOpen size={13} />}
                <span className="folder-row__name">{group.label}</span>
              </span>
              <span className="folder-row__meta">
                <span>{group.files.length}</span>
                <span className="delta-add">+{group.additions}</span>
                <span className="delta-del">−{group.deletions}</span>
                {notes > 0 ? (
                  <span
                    className="folder-row__comments"
                    title={`${notes} note${notes === 1 ? "" : "s"}`}
                  >
                    <MessageSquare size={9} /> {notes}
                  </span>
                ) : null}
              </span>
            </button>

            {collapsed
              ? null
              : group.files.map((file) => {
                  const notes = noteCounts[file.id] ?? 0;
                  const viewed = viewedFiles.has(file.id);
                  return (
                    <button
                      className={`file-row file-row--nested${selectedFileId === file.id ? " is-active" : ""}${viewed ? " is-viewed" : ""}`}
                      key={file.id}
                      onClick={() => onSelect(file.id)}
                      type="button"
                    >
                      <span
                        className={`file-row__status status-${file.status}`}
                      >
                        {viewed ? (
                          <Check size={11} />
                        ) : (
                          statusLetter(file.status)
                        )}
                      </span>
                      <span className="file-row__name-wrap">
                        <span className="file-row__name">
                          {filePathName(file)}
                        </span>
                        <span className="file-row__dir">
                          {filePathDir(file)}
                        </span>
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
          </div>
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
  collapsed,
  comments,
  composer,
  file,
  locked,
  onCancelComposer,
  onDeleteComment,
  onEditComment,
  onRendered,
  onSelectRange,
  onSubmitComposer,
  onToggleCollapsed,
  onToggleViewed,
  viewed,
}: {
  collapsed: boolean;
  comments: AppComment[];
  composer: ComposerState | null;
  file: DiffFile;
  locked: boolean;
  onCancelComposer: () => void;
  onDeleteComment: (id: string) => void;
  onEditComment: (id: string, body: string, severity: CommentSeverity) => void;
  onRendered: () => void;
  onSelectRange: (file: DiffFile, startKey: LineKey, endKey: LineKey) => void;
  onSubmitComposer: (body: string, severity: CommentSeverity) => void;
  onToggleCollapsed: (fileId: string) => void;
  onToggleViewed: (fileId: string) => void;
  viewed: boolean;
}) {
  const flat = useMemo(() => flattenFile(file), [file]);

  const composerKeys = useMemo(() => {
    if (composer === null || composer.fileId !== file.id) return null;
    const [start, end] = normalizeRange(composer.startKey, composer.endKey);
    return { start, end, keys: keysBetween(flat, start, end) };
  }, [composer, file.id, flat]);

  const annotations = useMemo(() => {
    const result: DiffLineAnnotation<DiffdeskAnnotation>[] = [];
    for (const comment of comments.filter((item) => item.fileId === file.id)) {
      const [, endKey] = normalizeRange(comment.startKey, comment.endKey);
      const anchor = annotationAnchorForKey(file, endKey);
      if (anchor === null) continue;
      result.push({ ...anchor, metadata: { kind: "comment", comment } });
    }
    if (composerKeys !== null) {
      const anchor = annotationAnchorForKey(file, composerKeys.end);
      if (anchor !== null) {
        result.push({
          ...anchor,
          metadata: {
            kind: "composer",
            rangeSize: composerKeys.keys.size || 1,
          },
        });
      }
    }
    return result;
  }, [comments, composerKeys, file]);

  const selectedLines = useMemo<SelectedLineRange | null>(() => {
    if (composerKeys === null) return null;
    return selectedLineRangeForKeys(file, composerKeys.start, composerKeys.end);
  }, [composerKeys, file]);

  return (
    <section
      className={`file${viewed ? " is-viewed" : ""}${collapsed ? " is-collapsed" : ""}`}
      data-file-id={file.id}
    >
      <header className="file__head">
        <button
          aria-expanded={!collapsed}
          className="file__head-toggle"
          onClick={() => onToggleCollapsed(file.id)}
          title={collapsed ? "Expand file" : "Collapse file"}
          type="button"
        >
          {collapsed ? <ChevronRight size={14} /> : <ChevronDown size={14} />}
          <span className="file__path">
            <span className="file__path-dir">{filePathDir(file)}/</span>
            <span className="file__path-name">{filePathName(file)}</span>
          </span>
        </button>
        <span className="file__head-stats">
          <span className={`type-badge status-badge status-${file.status}`}>
            {file.status}
          </span>
          <span className="type-mono delta-add">+{file.additions}</span>
          <span className="type-mono delta-del">−{file.deletions}</span>
          <label className="file__viewed" title="Mark file as viewed">
            <input
              checked={viewed}
              onChange={() => onToggleViewed(file.id)}
              type="checkbox"
            />
            <span>Viewed</span>
          </label>
        </span>
      </header>

      {file.tooLarge ? (
        <div className="large-diff-notice">
          <div>
            <div className="type-mono">Large diff</div>
            <div className="type-support">
              {file.additions + file.deletions} changed lines. The diff starts
              collapsed to keep the review responsive.
            </div>
          </div>
          {collapsed ? (
            <button
              className="btn btn--ghost"
              onClick={() => onToggleCollapsed(file.id)}
              type="button"
            >
              Render diff
            </button>
          ) : null}
        </div>
      ) : null}

      {collapsed ? null : (
        <div className="pierre-diff">
          <PatchDiff<DiffdeskAnnotation>
            disableWorkerPool
            lineAnnotations={annotations}
            patch={patchForFile(file)}
            renderAnnotation={(annotation) => {
              if (annotation.metadata.kind === "composer") {
                return (
                  <div className="thread thread--composer pierre-thread">
                    <Composer
                      rangeSize={annotation.metadata.rangeSize}
                      onCancel={onCancelComposer}
                      onSubmit={onSubmitComposer}
                    />
                  </div>
                );
              }
              const { comment } = annotation.metadata;
              return (
                <div className="thread pierre-thread">
                  <CommentBubble
                    comment={comment}
                    locked={locked}
                    onDelete={() => onDeleteComment(comment.id)}
                    onEdit={(body, severity) =>
                      onEditComment(comment.id, body, severity)
                    }
                  />
                </div>
              );
            }}
            selectedLines={selectedLines}
            options={{
              disableFileHeader: true,
              diffStyle: "unified",
              enableLineSelection: !locked,
              hunkSeparators: "metadata",
              lineHoverHighlight: "both",
              onLineSelected: (range) => {
                if (range === null || locked) return;
                const keys = keysForSelectedLineRange(file, range);
                if (keys === null) return;
                onSelectRange(file, keys[0], keys[1]);
              },
              // Find highlights are painted as CSS custom highlights over ranges
              // in this shadow root. Every render replaces the code's innerHTML
              // and invalidates those ranges, so they are repainted here.
              onPostRender: onRendered,
              overflow: "scroll",
              theme: "pierre-light",
              themeType: "light",
              // ::highlight() rules only apply inside the tree that owns them.
              unsafeCSS: findHighlightCSS,
            }}
          />
        </div>
      )}
    </section>
  );
}

function Composer({
  onCancel,
  onSubmit,
  rangeSize,
}: {
  onCancel: () => void;
  onSubmit: (body: string, severity: CommentSeverity) => void;
  rangeSize: number;
}) {
  // Draft text is kept local so each keystroke re-renders only this composer,
  // not the top-level App and every diff renderer below it. The text is lifted
  // to App state only on submit.
  const [draft, setDraft] = useState("");
  const [severity, setSeverity] = useState<CommentSeverity>("suggestion");
  const textAreaRef = useRef<HTMLTextAreaElement | null>(null);
  useEffect(() => textAreaRef.current?.focus(), []);
  const isEmpty = draft.trim() === "";
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
      <label className="severity-field">
        <span className="type-mono">Severity</span>
        <select
          aria-label="Comment severity"
          className="severity-select"
          onChange={(event) =>
            setSeverity(event.target.value as CommentSeverity)
          }
          value={severity}
        >
          {severityOptions.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </label>
      <textarea
        className="comment__textarea"
        onChange={(event) => setDraft(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Escape") {
            event.preventDefault();
            onCancel();
          }
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
            event.preventDefault();
            onSubmit(draft, severity);
          }
        }}
        placeholder="What should the agent change?"
        ref={textAreaRef}
        value={draft}
      />
      <div className="comment__actions">
        <button className="btn btn--ghost" onClick={onCancel} type="button">
          Cancel
        </button>
        <button
          className={`btn btn--primary${isEmpty ? " is-disabled" : ""}`}
          disabled={isEmpty}
          onClick={() => onSubmit(draft, severity)}
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
  onEdit: (body: string, severity: CommentSeverity) => void;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(comment.body);
  const [severity, setSeverity] = useState<CommentSeverity>(comment.severity);
  useEffect(() => {
    setDraft(comment.body);
    setSeverity(comment.severity);
  }, [comment.body, comment.severity]);

  if (editing) {
    return (
      <div className="comment comment--editing">
        <div className="comment__head">
          <span className="comment__author">{comment.author}</span>
          <span className="type-mono comment__when">Editing</span>
        </div>
        <label className="severity-field">
          <span className="type-mono">Severity</span>
          <select
            aria-label="Comment severity"
            className="severity-select"
            onChange={(event) =>
              setSeverity(event.target.value as CommentSeverity)
            }
            value={severity}
          >
            {severityOptions.map((option) => (
              <option key={option.value} value={option.value}>
                {option.label}
              </option>
            ))}
          </select>
        </label>
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
              onEdit(draft, severity);
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
        <span
          className={`type-badge severity-badge severity-${comment.severity}`}
        >
          {comment.severity}
        </span>
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

/**
 * True when the event target is a note textarea. Global shortcuts defer to it so
 * ⌘F and Escape do not yank the user out of a note they are writing.
 */
function isNoteEditor(target: EventTarget | null): boolean {
  return (
    target instanceof HTMLElement &&
    target.classList.contains("comment__textarea")
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
        severity: comment.severity ?? "suggestion",
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
      severity: comment.severity,
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

function patchForFile(file: DiffFile): string {
  const oldPath = file.oldPath === null ? "/dev/null" : `a/${file.oldPath}`;
  const newPath = file.newPath === null ? "/dev/null" : `b/${file.newPath}`;
  // The `diff --git` line must always carry real a/<path> b/<path> operands —
  // /dev/null there makes the @pierre/diffs parser throw and blanks the view.
  // Only the ---/+++ lines below may use /dev/null.
  const gitOldPath = `a/${file.oldPath ?? file.newPath}`;
  const gitNewPath = `b/${file.newPath ?? file.oldPath}`;
  return [
    `diff --git ${gitOldPath} ${gitNewPath}`,
    `--- ${oldPath}`,
    `+++ ${newPath}`,
    ...file.hunks.flatMap((hunk) => [
      hunk.header,
      ...hunk.lines.map((line) => line.raw),
    ]),
  ].join("\n");
}

function annotationAnchorForKey(
  file: DiffFile,
  key: LineKey,
): Pick<DiffLineAnnotation<DiffdeskAnnotation>, "lineNumber" | "side"> | null {
  const line = lineForKey(file, key);
  if (line === null) return null;
  if (line.kind === "deletion" && line.oldLineNumber !== null) {
    return { lineNumber: line.oldLineNumber, side: "deletions" };
  }
  if (line.newLineNumber !== null) {
    return { lineNumber: line.newLineNumber, side: "additions" };
  }
  if (line.oldLineNumber !== null) {
    return { lineNumber: line.oldLineNumber, side: "deletions" };
  }
  return null;
}

function selectedLineRangeForKeys(
  file: DiffFile,
  startKey: LineKey,
  endKey: LineKey,
): SelectedLineRange | null {
  const start = annotationAnchorForKey(file, startKey);
  const end = annotationAnchorForKey(file, endKey);
  if (start === null || end === null) return null;
  return {
    start: start.lineNumber,
    side: start.side,
    end: end.lineNumber,
    endSide: end.side,
  };
}

function keysForSelectedLineRange(
  file: DiffFile,
  range: SelectedLineRange,
): [LineKey, LineKey] | null {
  const start = keyForSelectedLine(file, range.start, range.side);
  const end = keyForSelectedLine(file, range.end, range.endSide ?? range.side);
  if (start === null || end === null) return null;
  return normalizeRange(start, end);
}

function keyForSelectedLine(
  file: DiffFile,
  lineNumber: number,
  side: SelectedLineRange["side"] = "additions",
): LineKey | null {
  const flat = flattenFile(file);
  const match = flat.find(({ line }) => {
    if (side === "deletions") return line.oldLineNumber === lineNumber;
    return line.newLineNumber === lineNumber;
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

function groupFilesByTopLevelFolder(files: DiffFile[]): SidebarFolderGroup[] {
  const groups = new Map<string, SidebarFolderGroup>();
  for (const file of files) {
    const folder = topLevelFolderForFile(file);
    const group = groups.get(folder.id) ?? {
      id: folder.id,
      label: folder.label,
      files: [],
      additions: 0,
      deletions: 0,
    };
    group.files.push(file);
    group.additions += file.additions;
    group.deletions += file.deletions;
    groups.set(folder.id, group);
  }
  return Array.from(groups.values());
}

function topLevelFolderForFile(file: DiffFile): { id: string; label: string } {
  const path = filePath(file);
  const [folder] = path.split("/");
  if (folder === undefined || folder === path) {
    return { id: ".", label: "Root" };
  }
  return { id: folder, label: folder };
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
