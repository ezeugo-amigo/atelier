use chrono::{DateTime, Utc};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Cell, Row, Table},
    Frame,
};
use vigil_core::{Session, SessionState};

use crate::app::App;

// Architecture color palette
const RED: Color = Color::Rgb(217, 119, 87);
const GOLD: Color = Color::Rgb(224, 184, 112);
const GREEN: Color = Color::Rgb(143, 191, 115);
const ACCENT: Color = Color::Rgb(212, 163, 115);
const DIM: Color = Color::DarkGray;

pub fn draw(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let [header, table, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(f, header, &app.sessions);
    draw_table(f, table, app);
    draw_footer(f, footer);
}

fn draw_header(f: &mut Frame, area: ratatui::layout::Rect, sessions: &[Session]) {
    let awaiting = sessions.iter().filter(|s| s.state.needs_attention()).count();
    let running = sessions
        .iter()
        .filter(|s| matches!(s.state, SessionState::Running))
        .count();

    let mut spans = vec![
        Span::styled("vigil", Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)),
        Span::styled(
            format!("  ·  {} sessions", sessions.len()),
            Style::default().fg(DIM),
        ),
    ];

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

fn draw_table(f: &mut Frame, area: ratatui::layout::Rect, app: &mut App) {
    let home = std::env::var("HOME").unwrap_or_default();

    let rows: Vec<Row> = app
        .sessions
        .iter()
        .map(|s| {
            let (dot, dot_style) = state_dot(&s.state);
            let state_str = state_label(&s.state);

            let project = s
                .project_dir
                .as_deref()
                .map(|p| {
                    let full = p.display().to_string();
                    if !home.is_empty() {
                        full.replacen(&home, "~", 1)
                    } else {
                        full
                    }
                })
                .unwrap_or_else(|| "-".into());

            let age = s
                .last_activity
                .map(fmt_age)
                .unwrap_or_else(|| "-".into());

            let msg = s
                .last_user_message
                .as_deref()
                .unwrap_or("-")
                .chars()
                .take(60)
                .collect::<String>();

            let state_style = state_label_style(&s.state);
            let text_style = Style::default().fg(Color::White);

            Row::new(vec![
                Cell::from(Span::styled(dot, dot_style)),
                Cell::from(Span::styled(state_str, state_style)),
                Cell::from(Span::styled(project, text_style)),
                Cell::from(Span::styled(age, text_style)),
                Cell::from(Span::styled(msg, text_style)),
            ])
        })
        .collect();

    let widths = [
        Constraint::Length(2),  // dot
        Constraint::Length(20), // state
        Constraint::Length(28), // project
        Constraint::Length(6),  // age
        Constraint::Fill(1),    // message
    ];

    let header = Row::new(["", "STATE", "PROJECT", "AGE", "LAST MESSAGE"])
        .style(Style::default().fg(DIM).add_modifier(Modifier::BOLD));

    let table = Table::new(rows, widths)
        .header(header)
        .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .block(Block::default());

    f.render_stateful_widget(table, area, &mut app.table_state);
}

fn draw_footer(f: &mut Frame, area: ratatui::layout::Rect) {
    let line = Line::from(vec![
        Span::styled("↑↓ jk", Style::default().fg(DIM)),
        Span::raw(" navigate  "),
        Span::styled("↵", Style::default().fg(DIM)),
        Span::raw(" attach  "),
        Span::styled("d", Style::default().fg(DIM)),
        Span::raw(" dismiss  "),
        Span::styled("q", Style::default().fg(DIM)),
        Span::raw(" quit"),
    ]);
    f.render_widget(line, area);
}

fn state_dot(state: &SessionState) -> (&'static str, Style) {
    match state {
        SessionState::AwaitingInput { .. } => ("●", Style::default().fg(RED)),
        SessionState::Running => ("◐", Style::default().fg(GOLD)),
        SessionState::Idle => ("○", Style::default().fg(GREEN)),
        SessionState::Done => ("·", Style::default().fg(DIM)),
        _ => ("?", Style::default().fg(DIM)),
    }
}

fn state_label(state: &SessionState) -> String {
    match state {
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

fn state_label_style(state: &SessionState) -> Style {
    match state {
        SessionState::AwaitingInput { .. } => Style::default().fg(RED),
        SessionState::Running => Style::default().fg(GOLD),
        SessionState::Idle => Style::default().fg(GREEN),
        SessionState::Done | SessionState::Unknown => Style::default().fg(DIM),
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
