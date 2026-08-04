use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionFile {
    pub schema_version: String,
    pub session_id: String,
    pub created_at: String,
    pub session_dir: PathBuf,
    pub input_diff_path: PathBuf,
    pub source: DiffSource,
    pub options: SessionOptions,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "kebab-case",
    rename_all_fields = "camelCase"
)]
pub enum DiffSource {
    Git {
        repo_root: PathBuf,
        working_directory: PathBuf,
        range: Option<String>,
        staged: bool,
        all: bool,
    },
    PatchFile {
        path: PathBuf,
    },
    Stdin,
    Raw {
        label: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionOptions {
    pub wait: bool,
    pub output_path: Option<PathBuf>,
    pub output_format: OutputFormat,
    pub copy_to_clipboard: bool,
    pub ai_command: Option<String>,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            wait: true,
            output_path: None,
            output_format: OutputFormat::Markdown,
            copy_to_clipboard: false,
            ai_command: None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum OutputFormat {
    Markdown,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DraftFile {
    pub schema_version: String,
    pub session_id: String,
    pub saved_at: String,
    pub summary: String,
    pub comments: Vec<ReviewComment>,
}

const REVIEW_STATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewStateFile {
    pub schema_version: u32,
    pub sources: BTreeMap<String, BTreeMap<String, FileReview>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileReview {
    pub fingerprint: String,
    pub reviewed_at: String,
}

impl Default for ReviewStateFile {
    fn default() -> Self {
        Self {
            schema_version: REVIEW_STATE_SCHEMA_VERSION,
            sources: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewComment {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub severity: CommentSeverity,
    pub body: String,
    pub anchor: CommentAnchor,
    pub context: CommentContext,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CommentSeverity {
    Note,
    Question,
    Suggestion,
    Issue,
    Blocking,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentAnchor {
    pub kind: String,
    pub file_id: String,
    pub path: String,
    pub side: String,
    pub old_line_number: Option<u32>,
    pub new_line_number: Option<u32>,
    pub line_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CommentContext {
    pub line: String,
    pub hunk_header: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitPayload {
    pub summary: String,
    pub comments: Vec<ReviewComment>,
    pub format: OutputFormat,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResult {
    pub schema_version: String,
    pub session_id: String,
    pub status: String,
    pub submitted_at: String,
    pub output_path: Option<PathBuf>,
    pub result_path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct CliRequest {
    pub raw_args: Vec<String>,
    pub current_dir: PathBuf,
    pub app_command: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedArgs {
    pub target: Option<String>,
    pub staged: bool,
    pub all: bool,
    pub wait: bool,
    pub output_path: Option<PathBuf>,
    pub output_format: OutputFormat,
    pub copy_to_clipboard: bool,
    pub ai_command: Option<String>,
    pub session_id: Option<String>,
    pub app_command: Option<String>,
    pub help: bool,
}

pub fn parse_args(raw_args: &[String]) -> Result<ParsedArgs> {
    let mut target = None;
    let mut staged = false;
    let mut all = false;
    let mut wait = true;
    let mut output_path = None;
    let mut output_format = OutputFormat::Markdown;
    let mut copy_to_clipboard = false;
    let mut ai_command = None;
    let mut session_id = None;
    let mut app_command = None;
    let mut help = false;

    let mut iter = raw_args.iter().peekable();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => help = true,
            "--staged" | "--cached" => staged = true,
            "--all" => all = true,
            "--wait" => wait = true,
            "--no-wait" => wait = false,
            "--copy" => copy_to_clipboard = true,
            "--json" => output_format = OutputFormat::Json,
            "--markdown" => output_format = OutputFormat::Markdown,
            "--format" => {
                let value = iter
                    .next()
                    .ok_or_else(|| anyhow!("--format requires markdown or json"))?;
                output_format = parse_format(value)?;
            }
            "--output" | "-o" => {
                let value = iter
                    .next()
                    .ok_or_else(|| anyhow!("--output requires a path"))?;
                output_path = Some(PathBuf::from(value));
            }
            "--session" => {
                let value = iter
                    .next()
                    .ok_or_else(|| anyhow!("--session requires an id"))?;
                session_id = Some(value.clone());
            }
            "--app-command" => {
                let value = iter
                    .next()
                    .ok_or_else(|| anyhow!("--app-command requires a command"))?;
                app_command = Some(value.clone());
            }
            "--send-to-ai" => {
                let value = iter
                    .next()
                    .ok_or_else(|| anyhow!("--send-to-ai requires a command"))?;
                ai_command = Some(value.clone());
            }
            value if value.starts_with('-') && value != "-" => {
                return Err(anyhow!("unknown argument: {value}"));
            }
            value => {
                if target.is_some() {
                    return Err(anyhow!(
                        "multiple diff targets supplied; use one range, file, or '-'"
                    ));
                }
                target = Some(value.to_string());
            }
        }
    }

    Ok(ParsedArgs {
        target,
        staged,
        all,
        wait,
        output_path,
        output_format,
        copy_to_clipboard,
        ai_command,
        session_id,
        app_command,
        help,
    })
}

fn parse_format(value: &str) -> Result<OutputFormat> {
    match value {
        "markdown" | "md" => Ok(OutputFormat::Markdown),
        "json" => Ok(OutputFormat::Json),
        other => Err(anyhow!(
            "unsupported format '{other}', expected markdown or json"
        )),
    }
}

pub fn help_text() -> &'static str {
    r#"Diffdesk - terminal-launched desktop diff review

Usage:
  diffdesk [range|patch-file|-] [options]

Examples:
  diffdesk
  diffdesk --staged
  diffdesk main...HEAD
  git diff | diffdesk -
  diffdesk changes.patch --output review.md
  diffdesk --wait --output review.json --format json

Options:
  --staged, --cached       Review staged changes
  --all                   Review all local changes against HEAD
  --wait / --no-wait      Wait for review completion (default: wait)
  -o, --output <path>     Write submitted review to path
  --format <markdown|json> Output format (default: markdown)
  --json / --markdown     Format shortcuts
  --copy                  Request copy-to-clipboard behavior in app
  --send-to-ai <command>  Store an AI command template for later integration
  --session <id>          Open an existing session (internal/app use)
  --app-command <command> CLI only: app executable/command to launch
"#
}

pub fn session_root() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not locate home directory"))?;
    Ok(home.join(".diffdesk").join("sessions"))
}

pub fn session_dir(session_id: &str) -> Result<PathBuf> {
    Ok(session_root()?.join(session_id))
}

pub fn review_state_path() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow!("could not locate home directory"))?;
    Ok(home.join(".diffdesk").join("review-state.json"))
}

pub fn load_review_state() -> Result<ReviewStateFile> {
    let path = review_state_path()?;
    if !path.exists() {
        return Ok(ReviewStateFile::default());
    }

    let json =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let state: ReviewStateFile = serde_json::from_str(&json)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    if state.schema_version != REVIEW_STATE_SCHEMA_VERSION {
        return Ok(ReviewStateFile::default());
    }
    Ok(state)
}

pub fn save_file_review(
    source_key: &str,
    file_key: &str,
    fingerprint: &str,
    reviewed: bool,
) -> Result<()> {
    let mut state = load_review_state()?;
    if reviewed {
        state
            .sources
            .entry(source_key.to_string())
            .or_default()
            .insert(
                file_key.to_string(),
                FileReview {
                    fingerprint: fingerprint.to_string(),
                    reviewed_at: Utc::now().to_rfc3339(),
                },
            );
    } else {
        let remove_source = state
            .sources
            .get_mut(source_key)
            .map(|files| {
                files.remove(file_key);
                files.is_empty()
            })
            .unwrap_or(false);
        if remove_source {
            state.sources.remove(source_key);
        }
    }

    write_review_state(&state)
}

pub fn flush_review_state() -> Result<()> {
    let state = load_review_state()?;
    write_review_state(&state)
}

fn write_review_state(state: &ReviewStateFile) -> Result<()> {
    let path = review_state_path()?;
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("review state path has no parent directory"))?;
    fs::create_dir_all(parent).with_context(|| format!("failed to create {}", parent.display()))?;
    let temp_path = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(state)?;
    fs::write(&temp_path, json)
        .with_context(|| format!("failed to write {}", temp_path.display()))?;
    fs::rename(&temp_path, &path).with_context(|| format!("failed to replace {}", path.display()))
}

pub fn create_session_from_args(args: &ParsedArgs, current_dir: &Path) -> Result<SessionFile> {
    if let Some(session_id) = &args.session_id {
        return load_session(session_id);
    }

    let session_id = format!("rev_{}", Uuid::new_v4().simple());
    let session_dir = session_dir(&session_id)?;
    fs::create_dir_all(&session_dir)
        .with_context(|| format!("failed to create session dir {}", session_dir.display()))?;

    let (raw_diff, source) = load_diff_source(args, current_dir)?;
    let input_diff_path = session_dir.join("input.diff");
    fs::write(&input_diff_path, raw_diff)
        .with_context(|| format!("failed to write {}", input_diff_path.display()))?;

    let session = SessionFile {
        schema_version: "1.0".to_string(),
        session_id: session_id.clone(),
        created_at: Utc::now().to_rfc3339(),
        session_dir: session_dir.clone(),
        input_diff_path,
        source,
        options: SessionOptions {
            wait: args.wait,
            output_path: args.output_path.clone(),
            output_format: args.output_format,
            copy_to_clipboard: args.copy_to_clipboard,
            ai_command: args.ai_command.clone(),
        },
    };

    write_session(&session)?;
    Ok(session)
}

pub fn write_session(session: &SessionFile) -> Result<()> {
    let path = session.session_dir.join("session.json");
    let json = serde_json::to_string_pretty(session)?;
    fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))
}

pub fn load_session(session_id: &str) -> Result<SessionFile> {
    let path = session_dir(session_id)?.join("session.json");
    let json =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    serde_json::from_str(&json).with_context(|| format!("failed to parse {}", path.display()))
}

pub fn read_input_diff(session_id: &str) -> Result<String> {
    let session = load_session(session_id)?;
    fs::read_to_string(&session.input_diff_path)
        .with_context(|| format!("failed to read {}", session.input_diff_path.display()))
}

pub fn load_drafts(session_id: &str) -> Result<Option<DraftFile>> {
    let path = session_dir(session_id)?.join("drafts.json");
    if !path.exists() {
        return Ok(None);
    }
    let json =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let drafts = serde_json::from_str(&json)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok(Some(drafts))
}

pub fn save_drafts(
    session_id: &str,
    summary: String,
    comments: Vec<ReviewComment>,
) -> Result<DraftFile> {
    let draft = DraftFile {
        schema_version: "1.0".to_string(),
        session_id: session_id.to_string(),
        saved_at: Utc::now().to_rfc3339(),
        summary,
        comments,
    };
    let path = session_dir(session_id)?.join("drafts.json");
    let json = serde_json::to_string_pretty(&draft)?;
    fs::write(&path, json).with_context(|| format!("failed to write {}", path.display()))?;
    Ok(draft)
}

pub fn submit_review(session_id: &str, payload: SubmitPayload) -> Result<SubmitResult> {
    let session = load_session(session_id)?;
    save_drafts(
        session_id,
        payload.summary.clone(),
        payload.comments.clone(),
    )?;

    let output_path = match session.options.output_path.clone() {
        Some(path) => Some(absolutize(&path)?),
        None => Some(session.session_dir.join(match payload.format {
            OutputFormat::Markdown => "review.md",
            OutputFormat::Json => "review.json",
        })),
    };

    let content = match payload.format {
        OutputFormat::Markdown => export_markdown(&session, &payload.summary, &payload.comments),
        OutputFormat::Json => serde_json::to_string_pretty(&payload)?,
    };

    if let Some(path) = &output_path {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("failed to create {}", parent.display()))?;
        }
        fs::write(path, &content).with_context(|| format!("failed to write {}", path.display()))?;
    }

    copy_text_to_clipboard(&content).context("failed to copy submitted review to clipboard")?;

    let result = SubmitResult {
        schema_version: "1.0".to_string(),
        session_id: session_id.to_string(),
        status: "submitted".to_string(),
        submitted_at: Utc::now().to_rfc3339(),
        output_path,
        result_path: session.session_dir.join("result.json"),
    };

    fs::write(&result.result_path, serde_json::to_string_pretty(&result)?)
        .with_context(|| format!("failed to write {}", result.result_path.display()))?;
    Ok(result)
}

pub fn copy_text_to_clipboard(text: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        return run_clipboard_command("pbcopy", &[], text);
    }

    #[cfg(target_os = "windows")]
    {
        return run_clipboard_command(
            "powershell.exe",
            &["-NoProfile", "-Command", "Set-Clipboard"],
            text,
        );
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let commands: [(&str, &[&str]); 3] = [
            ("wl-copy", &[]),
            ("xclip", &["-selection", "clipboard"]),
            ("xsel", &["--clipboard", "--input"]),
        ];
        let mut errors = Vec::new();
        for (program, args) in commands {
            match run_clipboard_command(program, args, text) {
                Ok(()) => return Ok(()),
                Err(error) => errors.push(format!("{program}: {error:#}")),
            }
        }
        return Err(anyhow!(
            "no clipboard command succeeded; tried wl-copy, xclip, and xsel ({})",
            errors.join("; ")
        ));
    }

    #[allow(unreachable_code)]
    Err(anyhow!("clipboard copy is not supported on this platform"))
}

fn run_clipboard_command(program: &str, args: &[&str], text: &str) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to start {program}"))?;

    {
        let stdin = child
            .stdin
            .as_mut()
            .ok_or_else(|| anyhow!("failed to open stdin for {program}"))?;
        stdin
            .write_all(text.as_bytes())
            .with_context(|| format!("failed to write clipboard text to {program}"))?;
    }

    let output = child
        .wait_with_output()
        .with_context(|| format!("failed to wait for {program}"))?;
    if !output.status.success() {
        return Err(anyhow!(
            "{program} exited with status {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    Ok(())
}

pub fn cancel_review(session_id: &str) -> Result<()> {
    let path = session_dir(session_id)?.join("canceled.json");
    let value = serde_json::json!({
        "schemaVersion": "1.0",
        "sessionId": session_id,
        "status": "canceled",
        "canceledAt": Utc::now().to_rfc3339()
    });
    fs::write(&path, serde_json::to_string_pretty(&value)?)
        .with_context(|| format!("failed to write {}", path.display()))
}

pub fn result_path(session_id: &str) -> Result<PathBuf> {
    Ok(session_dir(session_id)?.join("result.json"))
}

pub fn canceled_path(session_id: &str) -> Result<PathBuf> {
    Ok(session_dir(session_id)?.join("canceled.json"))
}

pub fn app_session_id_from_env_or_args(
    raw_args: &[String],
    current_dir: &Path,
) -> Result<SessionFile> {
    let parsed = parse_args(raw_args)?;
    create_session_from_args(&parsed, current_dir)
}

fn load_diff_source(args: &ParsedArgs, current_dir: &Path) -> Result<(String, DiffSource)> {
    match &args.target {
        Some(target) if target == "-" => {
            let mut raw = String::new();
            io::stdin()
                .read_to_string(&mut raw)
                .context("failed to read diff from stdin")?;
            Ok((raw, DiffSource::Stdin))
        }
        Some(target) => {
            let path = current_dir.join(target);
            if path.exists() && path.is_file() {
                let raw = fs::read_to_string(&path)
                    .with_context(|| format!("failed to read patch file {}", path.display()))?;
                Ok((raw, DiffSource::PatchFile { path }))
            } else {
                let repo_root = git_repo_root(current_dir)?;
                let raw = git_diff(current_dir, Some(target), args.staged, args.all)?;
                Ok((
                    raw,
                    DiffSource::Git {
                        repo_root,
                        working_directory: current_dir.to_path_buf(),
                        range: Some(target.clone()),
                        staged: args.staged,
                        all: args.all,
                    },
                ))
            }
        }
        None => {
            let repo_root = git_repo_root(current_dir)?;
            let raw = git_diff(current_dir, None, args.staged, args.all)?;
            Ok((
                raw,
                DiffSource::Git {
                    repo_root,
                    working_directory: current_dir.to_path_buf(),
                    range: None,
                    staged: args.staged,
                    all: args.all,
                },
            ))
        }
    }
}

fn git_repo_root(current_dir: &Path) -> Result<PathBuf> {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("--show-toplevel")
        .current_dir(current_dir)
        .output()
        .context("failed to execute git rev-parse")?;

    if !output.status.success() {
        return Err(anyhow!(
            "not inside a git repository: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(PathBuf::from(String::from_utf8(output.stdout)?.trim()))
}

fn git_diff(current_dir: &Path, range: Option<&str>, staged: bool, all: bool) -> Result<String> {
    let mut cmd = Command::new("git");
    cmd.arg("diff")
        .arg("--no-ext-diff")
        .arg("--src-prefix=a/")
        .arg("--dst-prefix=b/");

    if staged {
        cmd.arg("--cached");
    }

    if all {
        cmd.arg("HEAD");
    }

    if let Some(range) = range {
        cmd.arg(range);
    }

    let output = cmd
        .current_dir(current_dir)
        .output()
        .context("failed to execute git diff")?;
    if !output.status.success() {
        return Err(anyhow!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(String::from_utf8(output.stdout)?)
}

fn absolutize(path: &Path) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    Ok(env::current_dir()?.join(path))
}

pub fn spawn_app(command: &str, session_id: &str, wait_for_app_exit: bool) -> Result<()> {
    let mut parts = command.split_whitespace();
    let executable = parts.next().ok_or_else(|| anyhow!("empty app command"))?;
    let mut cmd = Command::new(executable);
    for part in parts {
        cmd.arg(part);
    }
    cmd.arg("--session").arg(session_id);

    if wait_for_app_exit {
        let status = cmd
            .status()
            .with_context(|| format!("failed to run app command '{command}'"))?;
        if !status.success() {
            return Err(anyhow!("app command exited with status {status}"));
        }
    } else {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        cmd.spawn()
            .with_context(|| format!("failed to spawn app command '{command}'"))?;
    }
    Ok(())
}

pub fn export_markdown(session: &SessionFile, summary: &str, comments: &[ReviewComment]) -> String {
    let mut out = String::new();
    out.push_str("# Diff Review Comments\n\n");
    out.push_str(&format!("Session: `{}`  \n", session.session_id));
    out.push_str(&format!("Created: `{}`  \n", session.created_at));
    out.push_str(&format!(
        "Source: `{}`\n\n",
        describe_source(&session.source)
    ));

    out.push_str("## Instructions for AI\n\n");
    out.push_str("You are receiving human review comments on a code diff. Apply the requested changes carefully, preserve unrelated behavior, and update tests when a comment identifies a correctness issue.\n\n");

    out.push_str("## Summary\n\n");
    if summary.trim().is_empty() {
        out.push_str("No global summary provided.\n\n");
    } else {
        out.push_str(summary.trim());
        out.push_str("\n\n");
    }

    out.push_str("## Comments\n\n");
    if comments.is_empty() {
        out.push_str("No inline comments.\n");
        return out;
    }

    for (index, comment) in comments.iter().enumerate() {
        out.push_str(&format!("### {}. `{}`\n\n", index + 1, comment.anchor.path));
        out.push_str(&format!("Comment ID: `{}`  \n", comment.id));
        out.push_str(&format!("Severity: `{:?}`  \n", comment.severity));
        out.push_str(&format!("Side: `{}`  \n", comment.anchor.side));
        match (
            comment.anchor.old_line_number,
            comment.anchor.new_line_number,
        ) {
            (_, Some(line)) => out.push_str(&format!("Line: `{line}`  \n")),
            (Some(line), None) => out.push_str(&format!("Old line: `{line}`  \n")),
            _ => {}
        }
        out.push('\n');
        out.push_str(comment.body.trim());
        out.push_str("\n\nRelevant line:\n\n```\n");
        out.push_str(&comment.context.line);
        out.push_str("\n```\n\n---\n\n");
    }

    out
}

fn describe_source(source: &DiffSource) -> String {
    match source {
        DiffSource::Git {
            repo_root,
            range,
            staged,
            all,
            ..
        } => {
            let mode = if *staged {
                "staged"
            } else if *all {
                "all local changes"
            } else {
                "working tree"
            };
            match range {
                Some(range) => format!("git {range} in {}", repo_root.display()),
                None => format!("git {mode} in {}", repo_root.display()),
            }
        }
        DiffSource::PatchFile { path } => format!("patch file {}", path.display()),
        DiffSource::Stdin => "stdin".to_string(),
        DiffSource::Raw { label } => label.clone(),
    }
}
