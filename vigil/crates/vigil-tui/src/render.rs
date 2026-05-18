use chrono::{DateTime, Utc};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table},
    Frame,
};
use vigil_core::SessionState;

use crate::app::{App, Overlay};

const RED: Color = Color::Rgb(217, 119, 87);
const GOLD: Color = Color::Rgb(224, 184, 112);
const GREEN: Color = Color::Rgb(143, 191, 115);
const ACCENT: Color = Color::Rgb(212, 163, 115);
const DIM: Color = Color::DarkGray;

const AGENT_LABELS: [&str; 4] = ["Claude", "Codex", "Pi", "OpenCode"];

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();

    let border = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::White));
    let inner = border.inner(area);
    f.render_widget(border, area);

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
    if !matches!(app.overlay, Overlay::None) {
        f.render_widget(Clear, inner);
    }

    match &app.overlay {
        Overlay::None => {}
        Overlay::NewWorktree { name_buf, agent_idx, repo_root } => {
            draw_new_worktree_overlay(f, area, name_buf, *agent_idx, repo_root.as_deref());
        }
        Overlay::DismissConfirm { container_id } => {
            draw_dismiss_confirm_overlay(f, area, container_id);
        }
        Overlay::RemoveConfirm { entry } => {
            draw_remove_confirm_overlay(f, area, &entry.id, &entry.worktree_path);
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
    let selected = app.table_state.selected();

    let rows: Vec<Row> = app
        .containers
        .iter()
        .enumerate()
        .map(|(i, c)| {
            let (dot, dot_style) = state_dot(&c.state);
            let state_str = state_label(&c.state);

            let project = {
                let full = c.worktree_path.display().to_string();
                if !home.is_empty() { full.replacen(&home, "~", 1) } else { full }
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

            Row::new(vec![
                bar,
                Cell::from(Span::styled(dot, dot_style)),
                Cell::from(state_str),
                Cell::from(c.id.clone()),
                Cell::from(project),
                Cell::from(age),
                Cell::from(msg),
            ])
            .style(row_style)
        })
        .collect();

    let widths = [
        Constraint::Length(1),  // accent bar
        Constraint::Length(2),  // dot
        Constraint::Length(20), // state
        Constraint::Length(20), // container id / branch name
        Constraint::Length(26), // path
        Constraint::Length(6),  // age
        Constraint::Fill(1),    // message
    ];

    let header = Row::new(["", "", "STATE", "CONTAINER", "PATH", "AGE", "LAST MESSAGE"])
        .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD));

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default());

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_footer(f: &mut Frame, area: Rect, app: &App) {
    let has_registry = app.registry.is_some();

    let mut spans = vec![
        Span::styled("↑↓ jk", Style::default().fg(DIM)),
        Span::raw(" navigate  "),
        Span::styled("↵", Style::default().fg(DIM)),
        Span::raw(" attach/launch  "),
        Span::styled("d", Style::default().fg(DIM)),
        Span::raw(" dismiss  "),
        Span::styled("u", Style::default().fg(DIM)),
        Span::raw(" undo  "),
    ];

    if has_registry {
        spans.push(Span::styled("W", Style::default().fg(DIM)));
        spans.push(Span::raw(" new  "));
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

fn centered_rect(percent_w: u16, height: u16, r: Rect) -> Rect {
    let popup_width = (r.width * percent_w / 100).max(1);
    Rect {
        x: r.x + (r.width.saturating_sub(popup_width)) / 2,
        y: r.y + (r.height.saturating_sub(height)) / 2,
        width: popup_width.min(r.width),
        height: height.min(r.height),
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
