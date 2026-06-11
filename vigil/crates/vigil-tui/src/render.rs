use chrono::{DateTime, Utc};
use ratatui::{
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};
use vigil_core::{AgentKind, BackgroundProcess, LogEvent, PrStatus, SessionState, ToolKind};

use crate::app::{input_presentation, App, Overlay, AGENTS};
use crate::recap::Recap;

const RED: Color = Color::Rgb(217, 119, 87);
const GOLD: Color = Color::Rgb(224, 184, 112);
const GREEN: Color = Color::Rgb(143, 191, 115);
const ACCENT: Color = Color::Rgb(212, 163, 115);
const PURPLE: Color = Color::Rgb(167, 139, 250);
const BLUE: Color = Color::Rgb(100, 149, 237);
const DIM: Color = Color::DarkGray;
const MUTED: Color = Color::Rgb(120, 112, 104);

const READ_COLOR: Color = Color::Rgb(74, 107, 138);
const BASH_COLOR: Color = Color::Rgb(122, 90, 138);
const EDIT_COLOR: Color = Color::Rgb(107, 138, 74);
const EMPTY_COLOR: Color = Color::Rgb(40, 40, 40);

const APP_MAX_WIDTH: u16 = 147;
const APP_MIN_WIDTH: u16 = 91;
const APP_HORIZONTAL_PADDING: u16 = 4;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();

    let inner = app_content_rect(area);

    let show_preview = app.selected().is_some() && inner.height >= 24;
    let preview_height = if show_preview {
        (inner.height / 3).clamp(8, 14)
    } else {
        0
    };

    let header_area;
    let table_area;
    let preview_area;
    let footer_area;
    if show_preview {
        let [h, t, p, f_] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(preview_height),
            Constraint::Length(1),
        ])
        .areas(inner);
        header_area = h;
        table_area = t;
        preview_area = Some(p);
        footer_area = f_;
    } else {
        let [h, t, f_] = Layout::vertical([
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .areas(inner);
        header_area = h;
        table_area = t;
        preview_area = None;
        footer_area = f_;
    }

    draw_header(f, header_area, app);
    draw_table(f, table_area, app);
    if let Some(p) = preview_area {
        draw_chat_preview(f, p, app);
    }
    draw_footer(f, footer_area, app);

    // Blank the interior before rendering any modal overlay so background content doesn't bleed through
    // SendMessage is inline (footer), not a modal, so skip the Clear for it.
    if !matches!(app.overlay, Overlay::None | Overlay::SendMessage { .. }) {
        f.render_widget(Clear, inner);
    }

    match &app.overlay {
        Overlay::None => {}
        Overlay::SendMessage {
            input,
            container_id,
            return_to_log,
        } => {
            let note = send_target_note(app, container_id.as_deref());
            let presentation = input_presentation(input);
            if *return_to_log {
                draw_log_response_overlay(
                    f,
                    area,
                    app,
                    container_id.as_deref(),
                    &presentation,
                    note.as_deref(),
                );
            } else {
                draw_send_message_overlay(f, area, &presentation, note.as_deref());
            }
        }
        Overlay::NewWorktree {
            name_buf,
            agent,
            repo_roots,
        } => {
            draw_new_worktree_overlay(f, area, name_buf, *agent, repo_roots);
        }
        Overlay::DismissConfirm { container_id } => {
            draw_dismiss_confirm_overlay(f, area, container_id);
        }
        Overlay::RemoveConfirm { entry } => {
            draw_remove_confirm_overlay(f, area, entry);
        }
        Overlay::LogView {
            container_id,
            events,
            lines,
            scroll,
            recap,
            recap_visible,
        } => {
            draw_log_view_overlay(
                f,
                area,
                container_id,
                events,
                lines,
                *scroll,
                recap,
                *recap_visible,
            );
        }
        Overlay::ProjectPicker {
            query,
            all_repos,
            selected_idx,
            scanning,
            checked,
        } => {
            draw_project_picker_overlay(
                f,
                area,
                query,
                all_repos,
                *selected_idx,
                *scanning,
                checked,
            );
        }
        Overlay::DefaultAgent { agent } => {
            draw_default_agent_overlay(f, area, *agent);
        }
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let containers = &app.containers;
    let awaiting = containers
        .iter()
        .filter(|c| c.state.needs_attention())
        .count();
    let running = containers
        .iter()
        .filter(|c| matches!(c.state, SessionState::Running))
        .count();
    let no_session = containers
        .iter()
        .filter(|c| matches!(c.state, SessionState::NoSession))
        .count();

    let mut spans = vec![
        Span::styled(
            "vigil",
            Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("  ·  {} containers", containers.len()),
            Style::default().fg(DIM),
        ),
    ];
    if no_session > 0 {
        spans.push(Span::styled(
            format!("  ·  {no_session} idle"),
            Style::default().fg(DIM),
        ));
    }
    if awaiting > 0 {
        spans.push(Span::styled(
            format!("  ·  {awaiting} awaiting"),
            Style::default().fg(RED).add_modifier(Modifier::BOLD),
        ));
    }
    if running > 0 {
        spans.push(Span::styled(
            format!("  ·  {running} running"),
            Style::default().fg(GOLD),
        ));
    }

    f.render_widget(Line::from(spans), area);
}

fn draw_table(f: &mut Frame, area: Rect, app: &mut App) {
    let home = std::env::var("HOME").unwrap_or_default();
    let vigil_wt = if home.is_empty() {
        "/.vigil/worktrees/".to_string()
    } else {
        format!("{home}/.vigil/worktrees/")
    };
    let selected = app.table_state.selected();

    // Build ordered groups: preserve first-seen order of repos.
    let mut group_order: Vec<String> = Vec::new();
    let mut groups: std::collections::HashMap<String, Vec<usize>> =
        std::collections::HashMap::new();
    for (i, c) in app.containers.iter().enumerate() {
        let repo = crate::app::container_repo_group(c, &vigil_wt);
        if !groups.contains_key(&repo) {
            group_order.push(repo.clone());
        }
        groups.entry(repo).or_default().push(i);
    }

    let mut rows: Vec<Row> = Vec::new();
    let mut container_to_row: Vec<usize> = vec![0; app.containers.len()];
    let message_width = last_message_column_width(area.width);

    for repo in &group_order {
        let indices = &groups[repo];

        // Separator row — repo name in white, dashes in dim.
        let dashes = 20_usize.saturating_sub(repo.len() + 4).max(2);
        rows.push(Row::new(vec![
            Cell::from(Span::styled("──", Style::default().fg(DIM))),
            Cell::from(""),
            Cell::from(Line::from(vec![
                Span::styled(" ".to_string(), Style::default()),
                Span::styled(
                    repo.clone(),
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(format!(" {}", "─".repeat(dashes)), Style::default().fg(DIM)),
            ])),
            Cell::from(Span::styled("───", Style::default().fg(DIM))),
            Cell::from(Span::styled("─".repeat(20), Style::default().fg(DIM))),
            Cell::from(Span::styled("───", Style::default().fg(DIM))),
            Cell::from(Span::styled("─".repeat(26), Style::default().fg(DIM))),
            Cell::from(Span::styled("──────", Style::default().fg(DIM))),
            Cell::from(Span::styled("─".repeat(50), Style::default().fg(DIM))),
            Cell::from(Span::styled("─".repeat(10), Style::default().fg(DIM))),
        ]));

        for &i in indices {
            container_to_row[i] = rows.len();
            let c = &app.containers[i];

            let (dot, dot_style) = state_dot(&c.state);
            let state_str = state_label(&c.state);

            // Vigil-managed worktrees show their live git branch; multi-repo
            // workspaces add a repo-count marker; external ones show the
            // worktree path (relative to home) since they aren't ours.
            let branch = if !c.repos.is_empty() {
                format!("{} ×{}", c.branch, c.repos.len())
            } else {
                let full = c.worktree_path.display().to_string();
                if full.starts_with(&vigil_wt) {
                    c.branch.clone()
                } else if !home.is_empty() {
                    full.replacen(&home, "~", 1)
                } else {
                    full
                }
            };

            let age = c
                .last_activity
                .or(Some(c.created_at))
                .map(fmt_age)
                .unwrap_or_else(|| "-".into());

            let msg = match &c.state {
                SessionState::NoSession => format!("↵ launch {}", c.agent.display_name()),
                _ => c.last_user_message.as_deref().unwrap_or("-").to_string(),
            };
            let msg_lines = wrap_str(&msg, message_width);
            let msg_height = msg_lines.len().max(1) as u16;
            let msg = msg_lines.join("\n");

            let row_style = state_style(&c.state);
            let bar = if selected == Some(i) {
                Cell::from(" ").style(Style::default().bg(ACCENT))
            } else {
                Cell::from(" ")
            };
            let agent_label = |agent: AgentKind| -> (&'static str, Style) {
                match agent {
                    AgentKind::ClaudeCode => ("◆", Style::default().fg(RED)),
                    AgentKind::Codex => ("◇", Style::default().fg(GOLD)),
                    AgentKind::Pi => ("π", Style::default().fg(BLUE)),
                    AgentKind::OpenCode => ("◎", Style::default().fg(GREEN)),
                    AgentKind::Droid => ("⬡", Style::default().fg(PURPLE)),
                }
            };
            let (agent_text, agent_style) = agent_label(c.agent);
            let (pr_icon, pr_style) = pr_dot(c.pr_status.as_ref());
            let (bg_icon, bg_style) = bg_task_dot(&c.background_processes);

            rows.push(
                Row::new(vec![
                    bar,
                    Cell::from(Span::styled(dot, dot_style)),
                    Cell::from(state_str),
                    Cell::from(Span::styled(bg_icon, bg_style)),
                    Cell::from(c.id.clone()),
                    Cell::from(Span::styled(pr_icon, pr_style)),
                    Cell::from(branch),
                    Cell::from(age),
                    Cell::from(msg),
                    Cell::from(Span::styled(format!(" {agent_text}"), agent_style)),
                ])
                .height(msg_height)
                .style(row_style),
            );
        }
    }

    // Remap container selection index → table row index for rendering.
    let mut render_state = app.table_state.clone();
    if let Some(ci) = selected {
        if let Some(&ri) = container_to_row.get(ci) {
            render_state.select(Some(ri));
        }
    }

    let widths = [
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(20),
        Constraint::Length(3),
        Constraint::Length(20),
        Constraint::Length(3),
        Constraint::Length(26),
        Constraint::Length(6),
        Constraint::Fill(1),
        Constraint::Length(10),
    ];

    let header = Row::new([
        "",
        "",
        "STATE",
        "",
        "CONTAINER",
        "",
        "BRANCH",
        "AGE",
        "LAST MESSAGE",
        " AGENT",
    ])
    .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(0)
        .block(Block::default());

    f.render_stateful_widget(table, area, &mut render_state);
}

fn last_message_column_width(table_width: u16) -> usize {
    // Fixed columns: selection bar, state dot, state, bg-task, container, PR, branch, age, agent.
    // The last-message column is the remaining Fill(1) column.
    const FIXED_COLUMNS_WIDTH: u16 = 1 + 2 + 20 + 3 + 20 + 3 + 26 + 6 + 10;
    table_width.saturating_sub(FIXED_COLUMNS_WIDTH).max(1) as usize
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let has_registry = app.registry.is_some();

    let mut spans = vec![
        Span::styled("↑↓ jk", Style::default().fg(DIM)),
        Span::raw(" navigate  "),
        Span::styled("↵", Style::default().fg(DIM)),
        Span::raw(" attach/launch  "),
        Span::styled("i", Style::default().fg(DIM)),
        Span::raw(" send  "),
        Span::styled("Tab", Style::default().fg(DIM)),
        Span::raw(" agent  "),
        Span::styled("l", Style::default().fg(DIM)),
        Span::raw(" log  "),
        Span::styled("d", Style::default().fg(DIM)),
        Span::raw(" dismiss  "),
        Span::styled("u", Style::default().fg(DIM)),
        Span::raw(" undo  "),
        Span::styled("o", Style::default().fg(DIM)),
        Span::raw(" terminal  "),
    ];

    if has_registry {
        spans.push(Span::styled("n", Style::default().fg(DIM)));
        spans.push(Span::raw(" new  "));
        spans.push(Span::styled("A", Style::default().fg(DIM)));
        spans.push(Span::raw(" open  "));
    }
    if app.can_remove_selected() {
        spans.push(Span::styled("R", Style::default().fg(DIM)));
        spans.push(Span::raw(" remove  "));
    }

    spans.push(Span::styled("S", Style::default().fg(DIM)));
    spans.push(Span::raw(" default  "));

    spans.push(Span::styled("q", Style::default().fg(DIM)));
    spans.push(Span::raw(" quit"));

    f.render_widget(Line::from(spans), area);
}

fn draw_send_message_overlay(f: &mut Frame, area: Rect, buf: &str, note: Option<&str>) {
    let popup = centered_rect(70, 12, area);
    draw_send_message_box(f, popup, " Send Message ", buf, note);
}

fn draw_send_message_box(f: &mut Frame, popup: Rect, title: &str, buf: &str, note: Option<&str>) {
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(popup).inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    f.render_widget(block, popup);

    let [content, hint_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);

    let mut lines = Vec::new();
    if let Some(note) = note {
        lines.push(Line::from(vec![
            Span::styled("replying to ", Style::default().fg(DIM)),
            Span::styled(note.to_string(), Style::default().fg(ACCENT)),
        ]));
        lines.push(Line::from(""));
    }
    lines.push(Line::from(Span::styled(
        format!("{buf}▋"),
        Style::default().fg(Color::White),
    )));

    f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), content);

    f.render_widget(
        Line::from(vec![
            Span::styled("Ctrl+Enter", Style::default().fg(DIM)),
            Span::raw(" newline  "),
            Span::styled("Enter", Style::default().fg(DIM)),
            Span::raw(" send  "),
            Span::styled("Esc", Style::default().fg(DIM)),
            Span::raw(" cancel"),
        ]),
        hint_area,
    );
}

fn agent_picker_spans(current: AgentKind) -> Vec<Span<'static>> {
    AGENTS
        .iter()
        .flat_map(|agent| {
            let (bullet, style) = if *agent == current {
                (
                    "◉ ",
                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                )
            } else {
                ("○ ", Style::default().fg(DIM))
            };
            [
                Span::styled(bullet, style),
                Span::styled(format!("{}  ", agent.display_name()), style),
            ]
        })
        .collect()
}

fn draw_new_worktree_overlay(
    f: &mut Frame,
    area: Rect,
    name_buf: &str,
    current: AgentKind,
    repo_roots: &[std::path::PathBuf],
) {
    // One line per extra repo beyond the first keeps every repo visible.
    let extra = repo_roots.len().saturating_sub(1) as u16;
    let popup = centered_rect(65, 11 + extra, area);
    f.render_widget(Clear, popup);

    let title = if repo_roots.len() > 1 {
        " New Workspace "
    } else {
        " New Container "
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let home = std::env::var("HOME").unwrap_or_default();
    let tilde = |p: &std::path::Path| {
        let s = p.display().to_string();
        if !home.is_empty() {
            s.replacen(&home, "~", 1)
        } else {
            s
        }
    };

    let agent_spans = agent_picker_spans(current);

    let mut lines: Vec<Line> = vec![Line::from("")];
    if repo_roots.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("  Repo   ", Style::default().fg(DIM)),
            Span::raw("(inferred from cwd)"),
        ]));
    } else {
        for (i, root) in repo_roots.iter().enumerate() {
            let label = if i == 0 { "  Repo   " } else { "         " };
            lines.push(Line::from(vec![
                Span::styled(label, Style::default().fg(DIM)),
                Span::raw(tilde(root)),
            ]));
        }
    }
    lines.extend([
        Line::from(vec![
            Span::styled("  Name   ", Style::default().fg(DIM)),
            Span::raw(format!("{name_buf}▋")),
        ]),
        Line::from(""),
        Line::from({
            let mut spans = vec![Span::styled("  Agent  ", Style::default().fg(DIM))];
            spans.extend(agent_spans);
            spans
        }),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Tab/S-Tab", Style::default().fg(DIM)),
            Span::raw(" cycle agent  "),
            Span::styled("Enter", Style::default().fg(DIM)),
            Span::raw(" create  "),
            Span::styled("Esc", Style::default().fg(DIM)),
            Span::raw(" cancel"),
        ]),
    ]);

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_default_agent_overlay(f: &mut Frame, area: Rect, current: AgentKind) {
    let popup = centered_rect(60, 9, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Default Agent ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let agent_spans = agent_picker_spans(current);

    let lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![Span::styled(
            "  Default agent for new containers",
            Style::default().fg(DIM),
        )]),
        Line::from(""),
        Line::from({
            let mut spans = vec![Span::raw("  ")];
            spans.extend(agent_spans);
            spans
        }),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("Tab/S-Tab", Style::default().fg(DIM)),
            Span::raw(" cycle  "),
            Span::styled("Enter", Style::default().fg(DIM)),
            Span::raw(" save  "),
            Span::styled("Esc", Style::default().fg(DIM)),
            Span::raw(" cancel"),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

#[allow(clippy::too_many_arguments)]
fn draw_log_view_overlay(
    f: &mut Frame,
    area: Rect,
    container_id: &str,
    events: &[LogEvent],
    lines: &[String],
    scroll: usize,
    recap: &Recap,
    recap_visible: bool,
) {
    let popup = centered_rect(92, area.height.saturating_sub(4), area);
    draw_log_view_panel(f, popup, container_id, events, lines, scroll);
    // Keep the corner clear until there's a recap to show, and honor the
    // user's hide toggle so it never permanently obscures the log text.
    if recap_visible && !matches!(recap, Recap::Idle) {
        draw_recap_box(f, popup, recap);
    }
}

fn draw_log_response_overlay(
    f: &mut Frame,
    area: Rect,
    app: &App,
    container_id: Option<&str>,
    buf: &str,
    note: Option<&str>,
) {
    let popup = centered_rect(92, area.height.saturating_sub(4), area);
    f.render_widget(Clear, popup);

    let input_height = 7.min(popup.height.saturating_sub(4)).max(3);
    let [log_area, input_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(input_height)]).areas(popup);

    let (id, events, lines) = container_id
        .and_then(|id| {
            app.cached_log_data(id)
                .map(|(events, lines)| (id, events, lines))
        })
        .unwrap_or(("?", &[] as &[LogEvent], &[] as &[String]));

    draw_log_view_panel(f, log_area, id, events, lines, 0);
    draw_send_message_box(f, input_area, " Reply ", buf, note);
}

fn draw_log_view_panel(
    f: &mut Frame,
    popup: Rect,
    container_id: &str,
    events: &[LogEvent],
    lines: &[String],
    scroll: usize,
) {
    f.render_widget(Clear, popup);

    let title = format!(" {} — log ", container_id);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(popup).inner(Margin {
        horizontal: 2,
        vertical: 1,
    });
    f.render_widget(block, popup);

    let [content, hint_area] =
        Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(inner);

    // Footer with hint + stats (for structured view).
    let turn_count = events
        .iter()
        .filter(|e| matches!(e, LogEvent::UserMessage { .. }))
        .count();
    let tool_count: u32 = events
        .iter()
        .filter_map(|e| {
            if let LogEvent::ToolGroup { tools } = e {
                Some(tools.iter().map(|(_, n)| *n).sum::<u32>())
            } else {
                None
            }
        })
        .sum();
    let mut hint_spans = vec![
        Span::styled("Esc / l", Style::default().fg(DIM)),
        Span::raw(" close  "),
        Span::styled("j/k", Style::default().fg(DIM)),
        Span::raw(" scroll  "),
        Span::styled("i", Style::default().fg(DIM)),
        Span::raw(" send  "),
        Span::styled("r", Style::default().fg(DIM)),
        Span::raw(" recap  "),
        Span::styled("R", Style::default().fg(DIM)),
        Span::raw(" hide/show"),
    ];
    if turn_count > 0 {
        hint_spans.push(Span::styled(
            format!("   turns {turn_count}  tools {tool_count}"),
            Style::default().fg(DIM),
        ));
    }
    f.render_widget(Line::from(hint_spans), hint_area);

    if !events.is_empty() {
        // ── Timeline view (structured JSONL sessions: Pi and future adapters) ──
        let display = build_event_lines(events, content.width as usize);

        let max_lines = content.height as usize;
        let max_scroll = display.len().saturating_sub(max_lines);
        let scroll = scroll.min(max_scroll);
        let start = display
            .len()
            .saturating_sub(max_lines)
            .saturating_sub(scroll);
        let visible: Vec<Line> = display.into_iter().skip(start).take(max_lines).collect();
        f.render_widget(Paragraph::new(visible), content);
    } else if !lines.is_empty() {
        // ── Raw log fallback (Claude Code debug logs) ─────────────────────────
        let max_lines = content.height as usize;
        let max_scroll = lines.len().saturating_sub(max_lines);
        let scroll = scroll.min(max_scroll);
        let start = lines.len().saturating_sub(max_lines).saturating_sub(scroll);
        let display: Vec<Line> = lines
            .iter()
            .skip(start)
            .take(max_lines)
            .map(|s| {
                let style = if s.contains(" ERR ") || s.contains("[ERR]") {
                    Style::default().fg(RED)
                } else if s.contains(" WRN ") || s.contains("[WRN]") {
                    Style::default().fg(GOLD)
                } else if s.contains(" INF ") || s.contains("[INF]") {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(DIM)
                };
                Line::from(Span::styled(
                    format!(" {}", sanitize_for_display(s)),
                    style,
                ))
            })
            .collect();
        f.render_widget(Paragraph::new(display), content);
    } else {
        f.render_widget(
            Paragraph::new("  no log data available").style(Style::default().fg(DIM)),
            content,
        );
    }
}

/// Render the LLM recap pinned to the bottom-right corner of the log popup.
fn draw_recap_box(f: &mut Frame, popup: Rect, recap: &Recap) {
    // Box geometry: roughly half the popup width, anchored bottom-right inside
    // the popup's border (the -1/-2 insets keep it off the frame edges).
    let box_width = (popup.width / 2).clamp(34, 60).min(popup.width.saturating_sub(4));
    let text_width = box_width.saturating_sub(4) as usize;

    let (title, body, color): (&str, String, Color) = match recap {
        Recap::Idle => (
            " recap ",
            "press r to summarize the last messages".to_string(),
            DIM,
        ),
        Recap::Loading => (" recap ", "summarizing…".to_string(), GOLD),
        Recap::Ready(text) => (" recap ", text.clone(), Color::White),
        Recap::Error(err) => (" recap ", format!("couldn't summarize: {err}"), RED),
    };

    let wrapped: Vec<Line> = body
        .lines()
        .flat_map(|raw| {
            let raw = if raw.is_empty() { " " } else { raw };
            wrap_str(raw, text_width)
                .into_iter()
                .map(|chunk| Line::from(render_inline(&chunk, color, EMPTY_COLOR)))
                .collect::<Vec<_>>()
        })
        .collect();

    // Cap height to half the popup so the box never swallows the whole log.
    let max_inner = (popup.height / 2).saturating_sub(2).max(1) as usize;
    let inner_lines = wrapped.len().clamp(1, max_inner);
    let box_height = (inner_lines as u16) + 2; // borders

    let x = popup.x + popup.width.saturating_sub(box_width).saturating_sub(2);
    let y = popup.y + popup.height.saturating_sub(box_height).saturating_sub(1);
    let rect = Rect {
        x,
        y,
        width: box_width,
        height: box_height,
    };

    f.render_widget(Clear, rect);
    let block = Block::default()
        .title(Span::styled(title, Style::default().fg(PURPLE)))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(PURPLE));
    let inner = block.inner(rect).inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    f.render_widget(block, rect);
    // Show the tail of the recap if it overflows the capped height.
    let start = wrapped.len().saturating_sub(inner_lines);
    let visible: Vec<Line> = wrapped.into_iter().skip(start).collect();
    f.render_widget(Paragraph::new(visible), inner);
}

fn build_event_lines(events: &[LogEvent], render_width: usize) -> Vec<Line<'static>> {
    let mut display: Vec<Line> = Vec::new();
    for event in events {
        match event {
            LogEvent::UserMessage { text, time } => {
                let time_str = time.as_deref().unwrap_or("");
                let prefix = "  YOU  ";
                let indent = " ".repeat(prefix.len());
                let text_width = render_width.saturating_sub(prefix.len());
                let mut first = true;
                for raw_line in text.lines() {
                    let raw_line = if raw_line.is_empty() { " " } else { raw_line };
                    for chunk in wrap_str(raw_line, text_width) {
                        if first {
                            display.push(Line::from(vec![
                                Span::styled(
                                    prefix,
                                    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(
                                    format!("{chunk}  "),
                                    Style::default().fg(Color::White),
                                ),
                                Span::styled(
                                    time_str.to_string(),
                                    Style::default().fg(EMPTY_COLOR),
                                ),
                            ]));
                            first = false;
                        } else {
                            display.push(Line::from(vec![
                                Span::raw(indent.clone()),
                                Span::styled(chunk, Style::default().fg(Color::White)),
                            ]));
                        }
                    }
                }
            }
            LogEvent::ToolGroup { tools } if !tools.is_empty() => {
                let bar_max = 12u32;
                let total_count: u32 = tools.iter().map(|(_, n)| *n).sum();
                let mut bar_spans: Vec<Span> =
                    vec![Span::styled(" TOOLS  ", Style::default().fg(DIM))];
                let mut used = 0u32;
                for (kind, count) in tools {
                    let blocks = ((count * bar_max + total_count / 2) / total_count).clamp(1, 4);
                    used += blocks;
                    let color = match kind {
                        ToolKind::Read => READ_COLOR,
                        ToolKind::Bash => BASH_COLOR,
                        ToolKind::Edit => EDIT_COLOR,
                        ToolKind::Other(_) => DIM,
                    };
                    bar_spans.push(Span::styled(
                        "\u{2588}".repeat(blocks as usize),
                        Style::default().fg(color),
                    ));
                }
                if used < bar_max {
                    bar_spans.push(Span::styled(
                        "\u{2591}".repeat((bar_max - used) as usize),
                        Style::default().fg(EMPTY_COLOR),
                    ));
                }
                bar_spans.push(Span::raw("  "));
                for (kind, count) in tools {
                    let color = match kind {
                        ToolKind::Read => READ_COLOR,
                        ToolKind::Bash => BASH_COLOR,
                        ToolKind::Edit => EDIT_COLOR,
                        ToolKind::Other(_) => DIM,
                    };
                    bar_spans.push(Span::styled(
                        format!("{}×{}  ", kind.label(), count),
                        Style::default().fg(color),
                    ));
                }
                display.push(Line::from(bar_spans));
            }
            LogEvent::AgentMessage { text, time, label } => {
                let time_str = time.as_deref().unwrap_or("");
                let prefix_str = format!("{:>5}  ", label.chars().take(5).collect::<String>());
                let prefix_len = prefix_str.len();
                let indent = " ".repeat(prefix_len);
                let text_width = render_width.saturating_sub(prefix_len);
                let mut first = true;
                for raw_line in text.lines() {
                    let raw_line = if raw_line.is_empty() { " " } else { raw_line };
                    for chunk in wrap_str(raw_line, text_width) {
                        let spans = render_inline(&chunk, GREEN, EMPTY_COLOR);
                        if first {
                            let mut line_spans = vec![Span::styled(
                                prefix_str.clone(),
                                Style::default().fg(GREEN).add_modifier(Modifier::BOLD),
                            )];
                            line_spans.extend(spans);
                            line_spans.push(Span::styled(
                                format!("  {time_str}"),
                                Style::default().fg(EMPTY_COLOR),
                            ));
                            display.push(Line::from(line_spans));
                            first = false;
                        } else {
                            let mut line_spans = vec![Span::raw(indent.clone())];
                            line_spans.extend(spans);
                            display.push(Line::from(line_spans));
                        }
                    }
                }
                display.push(Line::from(""));
            }
            _ => {}
        }
    }
    display
}

fn draw_chat_preview(f: &mut Frame, area: Rect, app: &App) {
    let Some(c) = app.selected() else { return };

    // Multi-repo workspaces get a per-repo PR glyph in the title.
    let mut title_spans = vec![Span::styled(
        format!(" {} — chat preview ", c.id),
        Style::default().fg(MUTED),
    )];
    if !c.repos.is_empty() {
        title_spans.push(Span::styled("· ", Style::default().fg(DIM)));
        for r in &c.repos {
            let name = r
                .repo_root
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("?");
            let (icon, style) = pr_dot(r.pr_status.as_ref());
            title_spans.push(Span::styled(format!("{name} "), Style::default().fg(MUTED)));
            title_spans.push(Span::styled(icon, style));
            title_spans.push(Span::raw(" "));
        }
    }
    let block = Block::default()
        .title(Line::from(title_spans))
        .borders(Borders::TOP)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(area).inner(Margin {
        horizontal: 1,
        vertical: 0,
    });
    f.render_widget(block, area);

    if inner.height == 0 || inner.width == 0 {
        return;
    }

    let data = app.cached_log_data(&c.id);
    match data {
        Some((events, _)) if !events.is_empty() => {
            let display = build_event_lines(events, inner.width as usize);
            let max_lines = inner.height as usize;
            let start = display.len().saturating_sub(max_lines);
            let visible: Vec<Line> = display.into_iter().skip(start).take(max_lines).collect();
            f.render_widget(Paragraph::new(visible), inner);
        }
        Some((_, lines)) if !lines.is_empty() => {
            let max_lines = inner.height as usize;
            let start = lines.len().saturating_sub(max_lines);
            let display: Vec<Line> = lines
                .iter()
                .skip(start)
                .take(max_lines)
                .map(|s| {
                    Line::from(Span::styled(
                        sanitize_for_display(s),
                        Style::default().fg(DIM),
                    ))
                })
                .collect();
            f.render_widget(Paragraph::new(display), inner);
        }
        _ => {
            f.render_widget(
                Paragraph::new(Span::styled(
                    "no chat data yet",
                    Style::default().fg(DIM),
                )),
                inner,
            );
        }
    }
}

fn draw_dismiss_confirm_overlay(f: &mut Frame, area: Rect, container_id: &str) {
    let popup = centered_rect(50, 7, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Dismiss Container ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(DIM));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Dismiss  "),
            Span::styled(
                container_id,
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  from view?"),
        ]),
        Line::from(vec![Span::styled(
            "  (undo with u)",
            Style::default().fg(DIM),
        )]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("y", Style::default().fg(GOLD).add_modifier(Modifier::BOLD)),
            Span::raw(" confirm  "),
            Span::styled("n / Esc", Style::default().fg(DIM)),
            Span::raw(" cancel"),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_remove_confirm_overlay(f: &mut Frame, area: Rect, entry: &atelier_worktree::WorktreeEntry) {
    // Workspaces list every contained checkout so it's clear what gets deleted.
    let extra = entry.repos.len().saturating_sub(1) as u16;
    let popup = centered_rect(55, 8 + extra, area);
    f.render_widget(Clear, popup);

    let title = if entry.is_workspace() {
        " Remove Workspace "
    } else {
        " Remove Container "
    };
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(RED));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let home = std::env::var("HOME").unwrap_or_default();
    let tilde = |p: &std::path::Path| {
        let s = p.display().to_string();
        if !home.is_empty() {
            s.replacen(&home, "~", 1)
        } else {
            s
        }
    };

    let mut lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Remove  "),
            Span::styled(
                entry.id.clone(),
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::raw("?"),
        ]),
    ];
    if entry.is_workspace() {
        for checkout in entry.checkouts() {
            lines.push(Line::from(vec![Span::styled(
                format!("  {}", tilde(&checkout.worktree_path)),
                Style::default().fg(DIM),
            )]));
        }
    } else {
        lines.push(Line::from(vec![Span::styled(
            format!("  {}", tilde(&entry.worktree_path)),
            Style::default().fg(DIM),
        )]));
    }
    lines.extend([
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("y", Style::default().fg(RED).add_modifier(Modifier::BOLD)),
            Span::raw(" confirm  "),
            Span::styled("n / Esc", Style::default().fg(DIM)),
            Span::raw(" cancel"),
        ]),
    ]);

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_project_picker_overlay(
    f: &mut Frame,
    area: Rect,
    query: &str,
    all_repos: &[std::path::PathBuf],
    selected_idx: usize,
    scanning: bool,
    checked: &[std::path::PathBuf],
) {
    let height = (area.height * 65 / 100).max(12).min(area.height);
    let popup = centered_rect(68, height, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Open Project ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let [search_area, divider_area, list_area, hint_area] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(inner);

    // Search input line
    f.render_widget(
        Line::from(vec![
            Span::styled(
                "▸ ",
                Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
            ),
            Span::styled(query.to_string(), Style::default().fg(Color::White)),
            Span::styled("_", Style::default().fg(ACCENT)),
        ]),
        search_area,
    );

    // Divider
    f.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(inner.width as usize),
            Style::default().fg(DIM),
        )),
        divider_area,
    );

    // Filtered list
    let home = std::env::var("HOME").unwrap_or_default();
    let q = query.to_lowercase();
    let filtered: Vec<&std::path::PathBuf> = all_repos
        .iter()
        .filter(|p| p.display().to_string().to_lowercase().contains(&q))
        .collect();

    if scanning && filtered.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("  scanning...", Style::default().fg(DIM))),
            list_area,
        );
    } else if filtered.is_empty() {
        f.render_widget(
            Paragraph::new(Span::styled("  no matches", Style::default().fg(DIM))),
            list_area,
        );
    } else {
        let max_visible = list_area.height as usize;
        let sel = selected_idx.min(filtered.len().saturating_sub(1));
        let scroll_start = sel.saturating_sub(max_visible.saturating_sub(1));

        let lines: Vec<Line> = filtered
            .iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(i, path)| {
                let is_selected = scroll_start + i == sel;
                let is_checked = checked.contains(path);
                let path_str = {
                    let s = path.display().to_string();
                    if !home.is_empty() {
                        s.replacen(&home, "~", 1)
                    } else {
                        s
                    }
                };
                // Check marker only appears once multi-select is in play, so
                // the classic single-select look is untouched.
                let mark = if checked.is_empty() {
                    ""
                } else if is_checked {
                    "◉ "
                } else {
                    "○ "
                };
                let cursor = if is_selected { "▸ " } else { "  " };
                let path_style = if is_selected {
                    Style::default()
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD)
                } else if is_checked {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(DIM)
                };
                Line::from(vec![
                    Span::styled(
                        cursor,
                        Style::default().fg(ACCENT).add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(mark, Style::default().fg(ACCENT)),
                    Span::styled(path_str, path_style),
                ])
            })
            .collect();

        f.render_widget(Paragraph::new(lines), list_area);
    }

    // Hint footer
    let count_str = if scanning {
        format!("{} repos (scanning...)", filtered.len())
    } else if checked.is_empty() {
        format!("{} / {} repos", filtered.len(), all_repos.len())
    } else {
        format!(
            "{} / {} repos · {} selected",
            filtered.len(),
            all_repos.len(),
            checked.len()
        )
    };
    let enter_hint = if checked.len() >= 2 {
        " create workspace  "
    } else {
        " open  "
    };
    f.render_widget(
        Line::from(vec![
            Span::styled(count_str, Style::default().fg(DIM)),
            Span::raw("   "),
            Span::styled("↑↓ / Tab", Style::default().fg(DIM)),
            Span::raw(" navigate  "),
            Span::styled("Space", Style::default().fg(DIM)),
            Span::raw(" select  "),
            Span::styled("Enter", Style::default().fg(DIM)),
            Span::raw(enter_hint),
            Span::styled("Esc", Style::default().fg(DIM)),
            Span::raw(" cancel"),
        ]),
        hint_area,
    );
}

fn send_target_note(app: &App, container_id: Option<&str>) -> Option<String> {
    let target = container_id
        .and_then(|id| app.containers.iter().find(|c| c.id == id))
        .or_else(|| app.selected());

    target.map(|c| c.branch.clone())
}

fn app_content_rect(area: Rect) -> Rect {
    let inner = area.inner(Margin {
        horizontal: 0,
        vertical: 1,
    });
    let padding = APP_HORIZONTAL_PADDING.min(inner.width.saturating_sub(APP_MIN_WIDTH) / 2);
    let padded = inner.inner(Margin {
        horizontal: padding,
        vertical: 0,
    });
    let width = padded.width.min(APP_MAX_WIDTH);

    Rect {
        x: padded.x + (padded.width.saturating_sub(width)) / 2,
        y: padded.y,
        width,
        height: padded.height,
    }
}

fn centered_rect(percent_w: u16, height: u16, r: Rect) -> Rect {
    let popup_width = (r.width * percent_w / 100).max(1);
    Rect {
        x: r.x + (r.width.saturating_sub(popup_width)) / 2,
        y: r.y + (r.height.saturating_sub(height)) / 2,
        width: popup_width.min(r.width),
        height: height.min(r.height),
    }
}

fn pr_dot(status: Option<&PrStatus>) -> (&'static str, Style) {
    match status {
        None | Some(PrStatus::NoPr) => ("   ", Style::default()),
        Some(PrStatus::InProgress) => (" ◯ ", Style::default().fg(GOLD)),
        Some(PrStatus::ReadyToMerge) => (" ◉ ", Style::default().fg(GREEN)),
        Some(PrStatus::Merged) => (" ● ", Style::default().fg(PURPLE)),
    }
}

fn bg_task_dot(procs: &[BackgroundProcess]) -> (&'static str, Style) {
    if procs.is_empty() {
        ("   ", Style::default())
    } else {
        (" ⚙ ", Style::default().fg(BLUE))
    }
}

fn state_dot(state: &SessionState) -> (&'static str, Style) {
    match state {
        SessionState::AwaitingInput { .. } => ("●", Style::default().fg(RED)),
        SessionState::Running => ("◐", Style::default().fg(GOLD)),
        SessionState::Idle => ("○", Style::default().fg(GREEN)),
        SessionState::Done => ("·", Style::default().fg(DIM)),
        SessionState::NoSession => ("·", Style::default().fg(DIM)),
        _ => ("?", Style::default().fg(DIM)),
    }
}

fn state_label(state: &SessionState) -> String {
    match state {
        SessionState::NoSession => "no session".into(),
        SessionState::AwaitingInput { reason: Some(r) } => {
            format!("awaiting: {}", r.chars().take(10).collect::<String>())
        }
        SessionState::AwaitingInput { reason: None } => "awaiting".into(),
        SessionState::Running => "running".into(),
        SessionState::Idle => "idle".into(),
        SessionState::Done => "done".into(),
        SessionState::Error { message: m } => {
            format!("error: {}", m.chars().take(13).collect::<String>())
        }
        SessionState::Unknown => "unknown".into(),
    }
}

fn state_style(state: &SessionState) -> Style {
    match state {
        SessionState::AwaitingInput { .. } => Style::default().fg(RED),
        SessionState::Running => Style::default().fg(GOLD),
        SessionState::Idle => Style::default().fg(GREEN),
        SessionState::NoSession | SessionState::Done | SessionState::Unknown => {
            Style::default().fg(DIM)
        }
        _ => Style::default().fg(Color::White),
    }
}

/// Strip ANSI escape sequences and replace other control characters.
///
/// Pasted or logged text frequently carries raw control codes — ANSI color
/// sequences, carriage returns, tabs, backspaces. If these reach ratatui's
/// cell buffer they are flushed verbatim to the terminal, which *interprets*
/// them: the cursor jumps, content spills outside the widget's rectangle, and
/// because ratatui's diff never wrote those off-region cells it never clears
/// them. The result is text that leaks into the margins and lingers until a
/// full redraw (restart). Sanitizing before render keeps every glyph a plain,
/// single-cell printable character.
fn sanitize_for_display(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\u{1b}' => match chars.peek() {
                // CSI: ESC [ … <final byte 0x40..=0x7e>
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('\u{40}'..='\u{7e}').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] … terminated by BEL or ST (ESC \)
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\u{07}' {
                            break;
                        }
                        if c == '\u{1b}' {
                            if matches!(chars.peek(), Some('\\')) {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                // Lone ESC or two-char escape — drop the following byte too.
                _ => {
                    chars.next();
                }
            },
            '\t' => out.push_str("    "),
            // Drop remaining control characters (CR, LF, backspace, etc.).
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Split `text` into chunks of at most `width` chars without breaking mid-word.
fn wrap_str(text: &str, width: usize) -> Vec<String> {
    let sanitized = sanitize_for_display(text);
    let text = sanitized.as_str();
    if width == 0 || text.is_empty() {
        return vec![text.to_string()];
    }
    let mut chunks = Vec::new();
    let mut remaining = text;
    while !remaining.is_empty() {
        let char_count = remaining.chars().count();
        if char_count <= width {
            chunks.push(remaining.to_string());
            break;
        }
        // Find last space within width chars
        let byte_end = remaining
            .char_indices()
            .nth(width)
            .map(|(i, _)| i)
            .unwrap_or(remaining.len());
        let slice = &remaining[..byte_end];
        let split = slice.rfind(' ').unwrap_or(byte_end);
        chunks.push(remaining[..split].to_string());
        remaining = remaining[split..].trim_start_matches(' ');
    }
    if chunks.is_empty() {
        chunks.push(String::new());
    }
    chunks
}

/// Parse a line for inline markdown — backtick `code` spans and `**bold**` —
/// and return styled Spans. Unterminated markers are emitted verbatim.
fn render_inline(text: &str, text_color: Color, _code_color: Color) -> Vec<Span<'static>> {
    const CODE_FG: Color = Color::Rgb(220, 200, 170);
    const CODE_BG: Color = Color::Rgb(45, 42, 38);

    let mut spans = Vec::new();
    let mut remaining = text;
    loop {
        // Find the earliest of the two inline markers we support.
        let code_at = remaining.find('`');
        let bold_at = remaining.find("**");
        let is_code = match (code_at, bold_at) {
            (None, None) => break,
            (Some(_), None) => true,
            (None, Some(_)) => false,
            (Some(c), Some(b)) => c <= b,
        };
        let pos = if is_code {
            code_at.unwrap()
        } else {
            bold_at.unwrap()
        };

        if pos > 0 {
            spans.push(Span::styled(
                remaining[..pos].to_string(),
                Style::default().fg(text_color),
            ));
        }

        if is_code {
            let rest = &remaining[pos + 1..];
            if let Some(end) = rest.find('`') {
                spans.push(Span::styled(
                    format!(" {} ", &rest[..end]),
                    Style::default().fg(CODE_FG).bg(CODE_BG),
                ));
                remaining = &rest[end + 1..];
            } else {
                spans.push(Span::styled(
                    format!("`{rest}"),
                    Style::default().fg(text_color),
                ));
                return spans;
            }
        } else {
            let rest = &remaining[pos + 2..];
            if let Some(end) = rest.find("**") {
                spans.push(Span::styled(
                    rest[..end].to_string(),
                    Style::default().fg(text_color).add_modifier(Modifier::BOLD),
                ));
                remaining = &rest[end + 2..];
            } else {
                spans.push(Span::styled(
                    format!("**{rest}"),
                    Style::default().fg(text_color),
                ));
                return spans;
            }
        }
    }
    if !remaining.is_empty() {
        spans.push(Span::styled(
            remaining.to_string(),
            Style::default().fg(text_color),
        ));
    }
    spans
}

fn fmt_age(ts: DateTime<Utc>) -> String {
    let secs = Utc::now().signed_duration_since(ts).num_seconds().max(0);
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        format!("{}m", secs / 60)
    } else {
        format!("{}h", secs / 3600)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_strips_ansi_color_sequences() {
        let input = "\u{1b}[31mred\u{1b}[0m and \u{1b}[1;32mgreen\u{1b}[m";
        assert_eq!(sanitize_for_display(input), "red and green");
    }

    #[test]
    fn sanitize_strips_osc_sequences() {
        // OSC terminated by BEL and by ST (ESC \).
        assert_eq!(
            sanitize_for_display("\u{1b}]0;window title\u{07}body"),
            "body"
        );
        assert_eq!(
            sanitize_for_display("\u{1b}]8;;http://x\u{1b}\\link"),
            "link"
        );
    }

    #[test]
    fn sanitize_drops_bare_control_chars_and_expands_tabs() {
        // CR, backspace, vertical tab, lone ESC are removed; tab → spaces.
        assert_eq!(sanitize_for_display("a\rb\x08c\x0bd"), "abcd");
        assert_eq!(sanitize_for_display("a\tb"), "a    b");
        assert_eq!(sanitize_for_display("a\nb"), "ab");
    }

    #[test]
    fn sanitize_keeps_plain_and_wide_unicode() {
        assert_eq!(sanitize_for_display("héllo 世界 ✓"), "héllo 世界 ✓");
    }

    #[test]
    fn wrap_str_sanitizes_before_wrapping() {
        // A line that is short once the escape codes are stripped must not be
        // forced onto multiple rows by the now-removed control bytes.
        let wrapped = wrap_str("\u{1b}[31mhello\u{1b}[0m", 10);
        assert_eq!(wrapped, vec!["hello".to_string()]);
    }
}
