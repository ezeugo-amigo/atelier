use chrono::{DateTime, Utc};
use ratatui::{
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
    Frame,
};
use vigil_core::{LogEvent, PrStatus, SessionState, ToolKind};

use crate::app::{App, Overlay};

const RED: Color = Color::Rgb(217, 119, 87);
const GOLD: Color = Color::Rgb(224, 184, 112);
const GREEN: Color = Color::Rgb(143, 191, 115);
const ACCENT: Color = Color::Rgb(212, 163, 115);
const PURPLE: Color = Color::Rgb(167, 139, 250);
const DIM: Color = Color::DarkGray;
const MUTED: Color = Color::Rgb(120, 112, 104);

const AGENT_LABELS: [&str; 4] = ["Claude", "Codex", "Pi", "OpenCode"];

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();

    let inner = area.inner(Margin { horizontal: 2, vertical: 1 });

    let [header, table, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(inner);

    draw_header(f, header, app);
    draw_table(f, table, app);
    draw_footer(f, footer, app);

    // Blank the interior before rendering any modal overlay so background content doesn't bleed through
    // SendMessage is inline (footer), not a modal, so skip the Clear for it.
    if !matches!(app.overlay, Overlay::None | Overlay::SendMessage { .. }) {
        f.render_widget(Clear, inner);
    }

    match &app.overlay {
        Overlay::None | Overlay::SendMessage { .. } => {}
        Overlay::NewWorktree { name_buf, agent_idx, repo_root } => {
            draw_new_worktree_overlay(f, area, name_buf, *agent_idx, repo_root.as_deref());
        }
        Overlay::DismissConfirm { container_id } => {
            draw_dismiss_confirm_overlay(f, area, container_id);
        }
        Overlay::RemoveConfirm { entry } => {
            draw_remove_confirm_overlay(f, area, &entry.id, &entry.worktree_path);
        }
        Overlay::LogView { container_id, events, lines, scroll, .. } => {
            draw_log_view_overlay(f, area, container_id, events, lines, *scroll);
        }
        Overlay::ProjectPicker { query, all_repos, selected_idx, scanning } => {
            draw_project_picker_overlay(f, area, query, all_repos, *selected_idx, *scanning);
        }
    }
}

fn draw_header(f: &mut Frame, area: Rect, app: &App) {
    let containers = &app.containers;
    let awaiting = containers.iter().filter(|c| c.state.needs_attention()).count();
    let running = containers
        .iter()
        .filter(|c| matches!(c.state, SessionState::Running))
        .count();
    let no_session = containers
        .iter()
        .filter(|c| matches!(c.state, SessionState::NoSession))
        .count();

    let mut spans = vec![
        Span::styled("vigil", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
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
    let mut groups: std::collections::HashMap<String, Vec<usize>> = std::collections::HashMap::new();
    for (i, c) in app.containers.iter().enumerate() {
        let full = c.worktree_path.display().to_string();
        let repo = if let Some(rest) = full.strip_prefix(&vigil_wt) {
            rest.split('/').next().unwrap_or("?").to_string()
        } else {
            c.repo_root.file_name().and_then(|n| n.to_str()).unwrap_or("?").to_string()
        };
        if !groups.contains_key(&repo) {
            group_order.push(repo.clone());
        }
        groups.entry(repo).or_default().push(i);
    }

    let mut rows: Vec<Row> = Vec::new();
    let mut container_to_row: Vec<usize> = vec![0; app.containers.len()];

    for repo in &group_order {
        let indices = &groups[repo];

        // Separator row — repo name in white, dashes in dim.
        let dashes = 20_usize.saturating_sub(repo.len() + 4).max(2);
        rows.push(Row::new(vec![
            Cell::from(Span::styled("──",                        Style::default().fg(DIM))),
            Cell::from(""),
            Cell::from(Line::from(vec![
                Span::styled(" ".to_string(),                    Style::default()),
                Span::styled(repo.clone(),                       Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" {}", "─".repeat(dashes)), Style::default().fg(DIM)),
            ])),
            Cell::from(Span::styled("─".repeat(20),   Style::default().fg(DIM))),
            Cell::from(Span::styled("───",            Style::default().fg(DIM))),
            Cell::from(Span::styled("─".repeat(26),   Style::default().fg(DIM))),
            Cell::from(Span::styled("──────",         Style::default().fg(DIM))),
            Cell::from(Span::styled("─".repeat(50),   Style::default().fg(DIM))),
        ]));

        for &i in indices {
            container_to_row[i] = rows.len();
            let c = &app.containers[i];

            let (dot, dot_style) = state_dot(&c.state);
            let state_str = state_label(&c.state);

            // Show only the branch name (last path component) within the group.
            let branch = {
                let full = c.worktree_path.display().to_string();
                if full.starts_with(&vigil_wt) {
                    c.worktree_path.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(c.branch.as_str())
                        .to_string()
                } else if !home.is_empty() {
                    full.replacen(&home, "~", 1)
                } else {
                    full
                }
            };

            let age = c.last_activity
                .or(Some(c.created_at))
                .map(fmt_age)
                .unwrap_or_else(|| "-".into());

            let msg = match &c.state {
                SessionState::NoSession => format!("↵ launch {}", c.agent.display_name()),
                _ => c.last_user_message.as_deref()
                    .unwrap_or("-")
                    .chars()
                    .take(60)
                    .collect::<String>(),
            };

            let row_style = state_style(&c.state);
            let bar = if selected == Some(i) {
                Cell::from(" ").style(Style::default().bg(ACCENT))
            } else {
                Cell::from(" ")
            };
            let (pr_icon, pr_style) = pr_dot(c.pr_status.as_ref());

            rows.push(Row::new(vec![
                bar,
                Cell::from(Span::styled(dot, dot_style)),
                Cell::from(state_str),
                Cell::from(c.id.clone()),
                Cell::from(Span::styled(pr_icon, pr_style)),
                Cell::from(branch),
                Cell::from(age),
                Cell::from(msg),
            ]).style(row_style));
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
        Constraint::Length(20),
        Constraint::Length(3),
        Constraint::Length(26),
        Constraint::Length(6),
        Constraint::Fill(1),
    ];

    let header = Row::new(["", "", "STATE", "CONTAINER", "PR", "BRANCH", "AGE", "LAST MESSAGE"])
        .style(Style::default().fg(MUTED).add_modifier(Modifier::BOLD));

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(0)
        .block(Block::default());

    f.render_stateful_widget(table, area, &mut render_state);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    if let Overlay::SendMessage { buf } = &app.overlay {
        let line = Line::from(vec![
            Span::styled("▸ ", Style::default().fg(ACCENT)),
            Span::raw(buf.as_str()),
            Span::styled("_", Style::default().fg(ACCENT)),
        ]);
        f.render_widget(line, area);
        return;
    }

    let has_registry = app.registry.is_some();

    let mut spans = vec![
        Span::styled("↑↓ jk", Style::default().fg(DIM)),
        Span::raw(" navigate  "),
        Span::styled("↵", Style::default().fg(DIM)),
        Span::raw(" attach/launch  "),
        Span::styled("i", Style::default().fg(DIM)),
        Span::raw(" send  "),
        Span::styled("a", Style::default().fg(DIM)),
        Span::raw(" agent  "),
        Span::styled("l", Style::default().fg(DIM)),
        Span::raw(" log  "),
        Span::styled("d", Style::default().fg(DIM)),
        Span::raw(" dismiss  "),
        Span::styled("u", Style::default().fg(DIM)),
        Span::raw(" undo  "),
    ];

    if has_registry {
        spans.push(Span::styled("W", Style::default().fg(DIM)));
        spans.push(Span::raw(" new  "));
        spans.push(Span::styled("A", Style::default().fg(DIM)));
        spans.push(Span::raw(" open  "));
    }
    if app.can_remove_selected() {
        spans.push(Span::styled("R", Style::default().fg(DIM)));
        spans.push(Span::raw(" remove  "));
    }

    spans.push(Span::styled("q", Style::default().fg(DIM)));
    spans.push(Span::raw(" quit"));

    f.render_widget(Line::from(spans), area);
}

fn draw_new_worktree_overlay(
    f: &mut Frame,
    area: Rect,
    name_buf: &str,
    agent_idx: usize,
    repo_root: Option<&std::path::Path>,
) {
    let popup = centered_rect(65, 11, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" New Container ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let home = std::env::var("HOME").unwrap_or_default();
    let repo_str = repo_root
        .map(|p| {
            let s = p.display().to_string();
            if !home.is_empty() { s.replacen(&home, "~", 1) } else { s }
        })
        .unwrap_or_else(|| "(inferred from cwd)".into());

    let agent_spans: Vec<Span> = AGENT_LABELS
        .iter()
        .enumerate()
        .flat_map(|(i, label)| {
            let (bullet, style) = if i == agent_idx {
                ("◉ ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD))
            } else {
                ("○ ", Style::default().fg(DIM))
            };
            [
                Span::styled(bullet, style),
                Span::styled(format!("{label}  "), style),
            ]
        })
        .collect();

    let lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Repo   ", Style::default().fg(DIM)),
            Span::raw(repo_str),
        ]),
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
            Span::styled("Tab", Style::default().fg(DIM)),
            Span::raw(" cycle agent  "),
            Span::styled("Enter", Style::default().fg(DIM)),
            Span::raw(" create  "),
            Span::styled("Esc", Style::default().fg(DIM)),
            Span::raw(" cancel"),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_log_view_overlay(
    f: &mut Frame,
    area: Rect,
    container_id: &str,
    events: &[LogEvent],
    lines: &[String],
    scroll: usize,
) {
    const READ_COLOR: Color = Color::Rgb(74, 107, 138);
    const BASH_COLOR: Color = Color::Rgb(122, 90, 138);
    const EDIT_COLOR: Color = Color::Rgb(107, 138, 74);
    const EMPTY_COLOR: Color = Color::Rgb(40, 40, 40);

    let popup = centered_rect(92, area.height.saturating_sub(4), area);
    f.render_widget(Clear, popup);

    let title = format!(" {} — log ", container_id);
    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(ACCENT));
    let inner = block.inner(popup).inner(Margin { horizontal: 2, vertical: 1 });
    f.render_widget(block, popup);

    let [content, hint_area] = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
    ]).areas(inner);

    // Footer with hint + stats (for structured view).
    let turn_count = events.iter().filter(|e| matches!(e, LogEvent::UserMessage { .. })).count();
    let tool_count: u32 = events.iter()
        .filter_map(|e| if let LogEvent::ToolGroup { tools } = e {
            Some(tools.iter().map(|(_, n)| *n).sum::<u32>())
        } else { None })
        .sum();
    let mut hint_spans = vec![
        Span::styled("Esc / l", Style::default().fg(DIM)),
        Span::raw(" close  "),
        Span::styled("j/k", Style::default().fg(DIM)),
        Span::raw(" scroll"),
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
        let render_width = content.width as usize;
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
                                    Span::styled(prefix, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                                    Span::styled(format!("{chunk}  "), Style::default().fg(Color::White)),
                                    Span::styled(time_str.to_string(), Style::default().fg(EMPTY_COLOR)),
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
                    let mut bar_spans: Vec<Span> = vec![
                        Span::styled(" TOOLS  ", Style::default().fg(DIM)),
                    ];
                    let mut used = 0u32;
                    for (kind, count) in tools {
                        let blocks = ((count * bar_max + total_count / 2) / total_count)
                            .max(1).min(4);
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
                LogEvent::AgentMessage { text, time } => {
                    let time_str = time.as_deref().unwrap_or("");
                    let prefix = "   PI  ";
                    let indent = " ".repeat(prefix.len());
                    let text_width = render_width.saturating_sub(prefix.len());
                    let mut first = true;
                    for raw_line in text.lines() {
                        let raw_line = if raw_line.is_empty() { " " } else { raw_line };
                        for chunk in wrap_str(raw_line, text_width) {
                            let spans = render_inline(&chunk, GREEN, EMPTY_COLOR);
                            if first {
                                let mut line_spans = vec![
                                    Span::styled(prefix, Style::default().fg(GREEN).add_modifier(Modifier::BOLD)),
                                ];
                                line_spans.extend(spans);
                                line_spans.push(Span::styled(format!("  {time_str}"), Style::default().fg(EMPTY_COLOR)));
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

        let max_lines = content.height as usize;
        let max_scroll = display.len().saturating_sub(max_lines);
        let scroll = scroll.min(max_scroll);
        let start = display.len().saturating_sub(max_lines).saturating_sub(scroll);
        let visible: Vec<Line> = display.into_iter().skip(start).take(max_lines).collect();
        f.render_widget(Paragraph::new(visible), content);

    } else if !lines.is_empty() {
        // ── Raw log fallback (Claude Code debug logs) ─────────────────────────
        let max_lines = content.height as usize;
        let max_scroll = lines.len().saturating_sub(max_lines);
        let scroll = scroll.min(max_scroll);
        let start = lines.len().saturating_sub(max_lines).saturating_sub(scroll);
        let display: Vec<Line> = lines.iter()
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
                Line::from(Span::styled(format!(" {s}"), style))
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
            Span::styled(container_id, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::raw("  from view?"),
        ]),
        Line::from(vec![Span::styled("  (undo with u)", Style::default().fg(DIM))]),
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

fn draw_remove_confirm_overlay(
    f: &mut Frame,
    area: Rect,
    container_id: &str,
    worktree_path: &std::path::Path,
) {
    let popup = centered_rect(55, 8, area);
    f.render_widget(Clear, popup);

    let block = Block::default()
        .title(" Remove Container ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(RED));
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let home = std::env::var("HOME").unwrap_or_default();
    let path_str = {
        let s = worktree_path.display().to_string();
        if !home.is_empty() { s.replacen(&home, "~", 1) } else { s }
    };

    let lines: Vec<Line> = vec![
        Line::from(""),
        Line::from(vec![
            Span::raw("  Remove  "),
            Span::styled(container_id, Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
            Span::raw("?"),
        ]),
        Line::from(vec![Span::styled(format!("  {path_str}"), Style::default().fg(DIM))]),
        Line::from(""),
        Line::from(vec![
            Span::raw("  "),
            Span::styled("y", Style::default().fg(RED).add_modifier(Modifier::BOLD)),
            Span::raw(" confirm  "),
            Span::styled("n / Esc", Style::default().fg(DIM)),
            Span::raw(" cancel"),
        ]),
    ];

    f.render_widget(Paragraph::new(lines), inner);
}

fn draw_project_picker_overlay(
    f: &mut Frame,
    area: Rect,
    query: &str,
    all_repos: &[std::path::PathBuf],
    selected_idx: usize,
    scanning: bool,
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
    ]).areas(inner);

    // Search input line
    f.render_widget(
        Line::from(vec![
            Span::styled("▸ ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
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
    let filtered: Vec<&std::path::PathBuf> = all_repos.iter()
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

        let lines: Vec<Line> = filtered.iter()
            .skip(scroll_start)
            .take(max_visible)
            .enumerate()
            .map(|(i, path)| {
                let is_selected = scroll_start + i == sel;
                let path_str = {
                    let s = path.display().to_string();
                    if !home.is_empty() { s.replacen(&home, "~", 1) } else { s }
                };
                if is_selected {
                    Line::from(vec![
                        Span::styled("▸ ", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
                        Span::styled(path_str, Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                    ])
                } else {
                    Line::from(vec![
                        Span::raw("  "),
                        Span::styled(path_str, Style::default().fg(DIM)),
                    ])
                }
            })
            .collect();

        f.render_widget(Paragraph::new(lines), list_area);
    }

    // Hint footer
    let count_str = if scanning {
        format!("{} repos (scanning...)", filtered.len())
    } else {
        format!("{} / {} repos", filtered.len(), all_repos.len())
    };
    f.render_widget(
        Line::from(vec![
            Span::styled(count_str, Style::default().fg(DIM)),
            Span::raw("   "),
            Span::styled("↑↓ / Tab", Style::default().fg(DIM)),
            Span::raw(" navigate  "),
            Span::styled("Enter", Style::default().fg(DIM)),
            Span::raw(" open  "),
            Span::styled("Esc", Style::default().fg(DIM)),
            Span::raw(" cancel"),
        ]),
        hint_area,
    );
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
        None | Some(PrStatus::NoPr) => ("  ", Style::default()),
        Some(PrStatus::InProgress)   => ("◯ ", Style::default().fg(GOLD)),
        Some(PrStatus::ReadyToMerge) => ("◉ ", Style::default().fg(GREEN)),
        Some(PrStatus::Merged)       => ("● ", Style::default().fg(PURPLE)),
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

/// Split `text` into chunks of at most `width` chars without breaking mid-word.
fn wrap_str(text: &str, width: usize) -> Vec<String> {
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
        let byte_end = remaining.char_indices().nth(width).map(|(i, _)| i).unwrap_or(remaining.len());
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

/// Parse a line for inline backtick code spans and return styled Spans.
fn render_inline(text: &str, text_color: Color, _code_color: Color) -> Vec<Span<'static>> {
    const CODE_FG: Color = Color::Rgb(220, 200, 170);
    const CODE_BG: Color = Color::Rgb(45, 42, 38);

    let mut spans = Vec::new();
    let mut remaining = text;
    while let Some(start) = remaining.find('`') {
        if !remaining[..start].is_empty() {
            spans.push(Span::styled(remaining[..start].to_string(), Style::default().fg(text_color)));
        }
        remaining = &remaining[start + 1..];
        if let Some(end) = remaining.find('`') {
            spans.push(Span::styled(
                format!(" {} ", &remaining[..end]),
                Style::default().fg(CODE_FG).bg(CODE_BG),
            ));
            remaining = &remaining[end + 1..];
        } else {
            spans.push(Span::styled(format!("`{remaining}"), Style::default().fg(text_color)));
            return spans;
        }
    }
    if !remaining.is_empty() {
        spans.push(Span::styled(remaining.to_string(), Style::default().fg(text_color)));
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
