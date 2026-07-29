//! All the drawing. Pure function of [`App`] state onto a frame.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Cell, Paragraph, Row, Table, TableState};

use crate::process::human_bytes;
use crate::tui::{App, Status};

/// One accent colour throughout, so the eye learns it in a second.
const ACCENT: Color = Color::Cyan;

const COLUMNS: [Constraint; 6] = [
    Constraint::Length(6),  // PORT
    Constraint::Length(5),  // PROTO
    Constraint::Length(8),  // PID
    Constraint::Min(12),    // PROCESS
    Constraint::Length(14), // USER
    Constraint::Length(9),  // MEMORY
];

pub fn draw(frame: &mut Frame, app: &mut App) {
    // The search line only earns its row while the filter is in play.
    let search_height = u16::from(app.searching() || !app.query().is_empty());
    let [header, body, search, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(search_height),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    draw_header(frame, header, app);
    if app.visible_count() == 0 {
        draw_empty(frame, body, app);
    } else {
        draw_table(frame, body, app);
    }
    if search_height > 0 {
        draw_search(frame, search, app);
    }
    draw_footer(frame, footer, app);
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let shown = app.visible_count();
    let total = app.total_count();
    // Only mention the filtered count when it differs, to keep the bar quiet.
    let count = if shown == total {
        format!(" {total} listening")
    } else {
        format!(" {shown} of {total} listening")
    };

    let left = Line::from(vec![
        Span::styled(
            " PORTKILL ",
            Style::new().fg(Color::Black).bg(ACCENT).bold(),
        ),
        Span::styled(count, Style::new().dim()),
    ]);

    frame.render_widget(Paragraph::new(left), area);
    frame.render_widget(
        Paragraph::new(Line::from(concat!("v", env!("CARGO_PKG_VERSION"), " ")).dim())
            .right_aligned(),
        area,
    );
}

fn draw_table(frame: &mut Frame, area: Rect, app: &mut App) {
    let header = Row::new(["PORT", "PROTO", "PID", "PROCESS", "USER", "MEMORY"]).style(
        Style::new()
            .fg(ACCENT)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    );

    let rows: Vec<Row> = app
        .visible()
        .map(|entry| {
            Row::new([
                Cell::from(entry.port.to_string()).bold(),
                Cell::from(entry.proto.to_string()).dim(),
                Cell::from(entry.pid.to_string()).dim(),
                Cell::from(entry.name.clone()),
                Cell::from(entry.user.clone()).dim(),
                Cell::from(Line::from(human_bytes(entry.memory)).right_aligned()),
            ])
        })
        .collect();

    let table = Table::new(rows, COLUMNS)
        .header(header)
        .column_spacing(1)
        // Inverted rather than merely tinted, so the cursor is unmistakable
        // on both light and dark terminals.
        .row_highlight_style(Style::new().fg(Color::Black).bg(ACCENT).bold())
        .highlight_symbol(Span::styled("▌", Style::new().fg(ACCENT)));

    let mut state = TableState::default().with_selected(app.selected());
    frame.render_stateful_widget(table, area, &mut state);
}

fn draw_empty(frame: &mut Frame, area: Rect, app: &App) {
    let message = if app.total_count() == 0 {
        "No listening ports found.".to_owned()
    } else {
        format!("No port or process matches “{}”.", app.query())
    };
    frame.render_widget(Paragraph::new(Line::from(message).dim()).centered(), area);
}

fn draw_search(frame: &mut Frame, area: Rect, app: &App) {
    let mut spans = vec![
        Span::styled(" / ", Style::new().fg(Color::Black).bg(ACCENT).bold()),
        Span::raw(" "),
        Span::raw(app.query().to_owned()),
    ];
    if app.searching() {
        // A block cursor, since we never move the real one.
        spans.push(Span::styled(
            "█",
            Style::new().fg(ACCENT).add_modifier(Modifier::SLOW_BLINK),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_footer(frame: &mut Frame, area: Rect, app: &App) {
    let line = match app.status() {
        Some(Status::Info(text)) => Line::from(vec![
            Span::styled(" ✓ ", Style::new().fg(Color::Black).bg(Color::Green).bold()),
            Span::styled(format!(" {text}"), Style::new().fg(Color::Green)),
        ]),
        Some(Status::Error(text)) => Line::from(vec![
            Span::styled(" ✗ ", Style::new().fg(Color::Black).bg(Color::Red).bold()),
            Span::styled(format!(" {text}"), Style::new().fg(Color::Red)),
        ]),
        None if app.searching() => hints(&[("⏎", "apply"), ("esc", "clear"), ("↑↓", "move")]),
        None => hints(&[
            ("j/k", "move"),
            ("/", "search"),
            ("⏎", "SIGTERM"),
            ("K", "SIGKILL"),
            ("r", "refresh"),
            ("q", "quit"),
        ]),
    };
    frame.render_widget(Paragraph::new(line), area);
}

/// Render `key label` pairs separated by dots, keys highlighted.
fn hints(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = Vec::with_capacity(pairs.len() * 4);
    for (key, label) in pairs {
        if !spans.is_empty() {
            spans.push(Span::styled(" · ", Style::new().dim()));
        } else {
            spans.push(Span::raw(" "));
        }
        spans.push(Span::styled(
            (*key).to_owned(),
            Style::new().fg(ACCENT).bold(),
        ));
        spans.push(Span::styled(format!(" {label}"), Style::new().dim()));
    }
    Line::from(spans)
}
