use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Scrollbar, ScrollbarOrientation, ScrollbarState, Table};

use super::{App, Focus, JobStatus, Screen};
use crate::util::format_sol_compact;

pub fn render(f: &mut Frame, app: &mut App) {
    match app.screen {
        Screen::Main => render_main(f, app),
        Screen::JobDetail(idx) => render_detail(f, app, idx),
    }
}

fn render_main(f: &mut Frame, app: &mut App) {
    let area = f.area();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2), // header
            Constraint::Percentage(50), // table
            Constraint::Min(4),   // logs
            Constraint::Length(1), // help bar
        ])
        .split(area);

    // ── Header ──
    let price_str = if app.free_mode {
        "FREE".to_string()
    } else {
        format!("{} SOL", format_sol_compact(app.price))
    };
    let header_line1 = Line::from(vec![
        Span::styled("  ⚡ ELISYM", Style::default().fg(Color::Yellow).bold()),
        Span::styled("  agent: ", Style::default().fg(Color::DarkGray)),
        Span::styled(&app.agent_name, Style::default().fg(Color::White).bold()),
        Span::styled("  skill: ", Style::default().fg(Color::DarkGray)),
        Span::styled(&app.skill_name, Style::default().fg(Color::Cyan).bold()),
    ]);
    let header_line2 = Line::from(vec![
        Span::styled("     price: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            &price_str,
            if app.free_mode {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default().fg(Color::Green).bold()
            },
        ),
        Span::styled("  wallet: ", Style::default().fg(Color::DarkGray)),
        Span::styled(
            format!("{} SOL", format_sol_compact(app.wallet_balance)),
            Style::default().fg(Color::Green),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(&app.network, Style::default().fg(Color::Cyan)),
    ]);
    let header = Paragraph::new(vec![header_line1, header_line2]);
    f.render_widget(header, chunks[0]);

    // ── Job table ──
    let table_focus = matches!(app.focus, Focus::Table);
    let table_border_style = if table_focus {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let job_count = app.jobs.len();
    let title = if job_count == 0 {
        " Jobs ".to_string()
    } else {
        let running = app.jobs.iter().filter(|j| matches!(j.status, JobStatus::Processing)).count();
        let done = app.jobs.iter().filter(|j| matches!(j.status, JobStatus::Completed)).count();
        let failed = app.jobs.iter().filter(|j| matches!(j.status, JobStatus::Failed(_))).count();
        format!(" Jobs ({}) — {} running, {} done, {} failed ", job_count, running, done, failed)
    };

    let header_row = Row::new(vec![
        Cell::from(" # ").style(Style::default().fg(Color::DarkGray)),
        Cell::from("Job ID"),
        Cell::from("From"),
        Cell::from("Status"),
        Cell::from("Skill"),
        Cell::from("  Time"),
        Cell::from("    SOL"),
    ])
    .style(Style::default().bold().fg(Color::White))
    .bottom_margin(0);

    let rows: Vec<Row> = app
        .jobs
        .iter()
        .enumerate()
        .map(|(i, job)| {
            let short_id = if job.job_id.len() > 10 {
                format!("{}…", &job.job_id[..10])
            } else {
                job.job_id.clone()
            };
            let short_customer = if job.customer_id.len() > 10 {
                format!("{}…", &job.customer_id[..10])
            } else {
                job.customer_id.clone()
            };

            let elapsed = job
                .completed_at
                .unwrap_or_else(std::time::Instant::now)
                .duration_since(job.started_at);
            let secs = elapsed.as_secs();
            let time_str = if secs >= 60 {
                format!("{:>2}m{:02}s", secs / 60, secs % 60)
            } else {
                format!("{:>4}s", secs)
            };

            let sol_str = job
                .price
                .map(format_sol_compact)
                .unwrap_or_else(|| "   --".into());

            let (status_text, status_style) = match &job.status {
                JobStatus::PaymentPending => ("$ Awaiting", Style::default().fg(Color::Yellow)),
                JobStatus::Processing => ("⚙ Running", Style::default().fg(Color::Cyan)),
                JobStatus::Completed => ("✓ Done", Style::default().fg(Color::Green)),
                JobStatus::Failed(_) => ("✗ Failed", Style::default().fg(Color::Red)),
            };

            let skill_str = job.skill_name.as_deref().unwrap_or("—");

            let row_style = if i % 2 == 1 {
                Style::default().bg(Color::Rgb(30, 30, 40))
            } else {
                Style::default()
            };

            Row::new(vec![
                Cell::from(format!("{:>2}", i + 1)).style(Style::default().fg(Color::DarkGray)),
                Cell::from(short_id).style(Style::default().fg(Color::White)),
                Cell::from(short_customer).style(Style::default().fg(Color::DarkGray)),
                Cell::from(status_text).style(status_style),
                Cell::from(skill_str).style(Style::default().fg(Color::Cyan)),
                Cell::from(time_str).style(Style::default().fg(Color::DarkGray)),
                Cell::from(format!("{:>7}", sol_str)).style(Style::default().fg(Color::Yellow)),
            ])
            .style(row_style)
        })
        .collect();

    let table = Table::new(
        rows,
        [
            Constraint::Length(4),       // #
            Constraint::Min(14),         // Job ID (expands)
            Constraint::Min(14),         // From (expands)
            Constraint::Length(11),      // Status
            Constraint::Length(18),      // Skill
            Constraint::Length(7),       // Time
            Constraint::Length(9),       // SOL
        ],
    )
    .header(header_row)
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(table_border_style)
            .title(title),
    )
    .row_highlight_style(
        Style::default()
            .bg(Color::Rgb(40, 50, 70))
            .fg(Color::White)
            .add_modifier(Modifier::BOLD),
    );

    f.render_stateful_widget(table, chunks[1], &mut app.table_state);

    // Empty state message
    if app.jobs.is_empty() {
        let inner = chunks[1].inner(Margin::new(1, 1));
        let empty = Paragraph::new("  Waiting for jobs…")
            .style(Style::default().fg(Color::DarkGray).italic())
            .alignment(Alignment::Left);
        // Render below the header row
        if inner.height > 2 {
            let empty_area = Rect {
                x: inner.x,
                y: inner.y + 1,
                width: inner.width,
                height: 1,
            };
            f.render_widget(empty, empty_area);
        }
    }

    // ── Logs ──
    let log_focus = matches!(app.focus, Focus::Log);
    let log_border_style = if log_focus {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let log_lines: Vec<Line> = app
        .global_logs
        .iter()
        .map(|l| {
            Line::from(vec![
                Span::styled(format!("  {} ", l.time), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} ", l.icon), icon_style(l.icon)),
                Span::raw(&l.message),
            ])
        })
        .collect();

    let log_height = chunks[2].height.saturating_sub(2) as usize;
    let total_lines = log_lines.len();
    let max_scroll = total_lines.saturating_sub(log_height) as u16;

    // Auto-scroll to bottom only when NOT focused on log pane
    if !log_focus {
        app.log_scroll = max_scroll;
    }
    // Clamp scroll to valid range
    if app.log_scroll > max_scroll {
        app.log_scroll = max_scroll;
    }

    let log_paragraph = Paragraph::new(log_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(log_border_style)
                .title(" Log "),
        )
        .scroll((app.log_scroll, 0));

    f.render_widget(log_paragraph, chunks[2]);

    if total_lines > log_height {
        let mut scrollbar_state = ScrollbarState::new(max_scroll as usize)
            .position(app.log_scroll as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            chunks[2],
            &mut scrollbar_state,
        );
    }

    // ── Help bar ──
    let sound_label = if app.sound_enabled { "sound:on" } else { "sound:off" };
    let help = Line::from(vec![
        Span::styled("  ↑↓", Style::default().fg(Color::White).bold()),
        Span::styled(" select  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Enter", Style::default().fg(Color::White).bold()),
        Span::styled(" detail  ", Style::default().fg(Color::DarkGray)),
        Span::styled("Tab", Style::default().fg(Color::White).bold()),
        Span::styled(" switch pane  ", Style::default().fg(Color::DarkGray)),
        Span::styled("s", Style::default().fg(Color::White).bold()),
        Span::styled(
            format!(" {}  ", sound_label),
            if app.sound_enabled {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::DarkGray)
            },
        ),
        Span::styled("q", Style::default().fg(Color::White).bold()),
        Span::styled(" quit", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(help), chunks[3]);
}

fn render_detail(f: &mut Frame, app: &mut App, job_idx: usize) {
    let area = f.area();

    let job = match app.jobs.get(job_idx) {
        Some(j) => j,
        None => {
            app.screen = Screen::Main;
            return;
        }
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(9), // info
            Constraint::Min(5),   // logs
            Constraint::Length(1), // help
        ])
        .split(area);

    // ── Info block ──
    let elapsed = job
        .completed_at
        .unwrap_or_else(std::time::Instant::now)
        .duration_since(job.started_at);

    let price_str = job
        .price
        .map(|p| format!("{} SOL", format_sol_compact(p)))
        .unwrap_or_else(|| "—".into());
    let net_str = job
        .net_amount
        .map(|n| format!(" (net: {} SOL)", format_sol_compact(n)))
        .unwrap_or_default();

    let input_preview = if job.input.len() > 60 {
        format!("{}…", &job.input[..60])
    } else {
        job.input.clone()
    };
    let input_preview = input_preview.replace('\n', " ");

    let secs = elapsed.as_secs();
    let duration_str = if secs >= 60 {
        format!("{}m{}s", secs / 60, secs % 60)
    } else {
        format!("{}s", secs)
    };

    let info_text = vec![
        Line::from(vec![
            Span::styled("  From:     ", Style::default().fg(Color::DarkGray)),
            Span::raw(&job.customer_id),
        ]),
        Line::from(vec![
            Span::styled("  Status:   ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                job.status.to_string(),
                match &job.status {
                    JobStatus::PaymentPending => Style::default().fg(Color::Yellow),
                    JobStatus::Processing => Style::default().fg(Color::Cyan),
                    JobStatus::Completed => Style::default().fg(Color::Green),
                    JobStatus::Failed(_) => Style::default().fg(Color::Red),
                },
            ),
        ]),
        Line::from(vec![
            Span::styled("  Skill:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                job.skill_name.as_deref().unwrap_or("—"),
                Style::default().fg(Color::Cyan),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Input:    ", Style::default().fg(Color::DarkGray)),
            Span::raw(input_preview),
        ]),
        Line::from(vec![
            Span::styled("  Price:    ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}{}", price_str, net_str),
                Style::default().fg(Color::Yellow),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Duration: ", Style::default().fg(Color::DarkGray)),
            Span::raw(duration_str),
        ]),
    ];

    let short_id = if job.job_id.len() > 16 {
        format!("{}…", &job.job_id[..16])
    } else {
        job.job_id.clone()
    };

    let info = Paragraph::new(info_text).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(format!(" Job {} ", short_id))
            .title_bottom(" Esc to back "),
    );
    f.render_widget(info, chunks[0]);

    // ── Detail logs ──
    let detail_lines: Vec<Line> = job
        .logs
        .iter()
        .map(|l| {
            Line::from(vec![
                Span::styled(format!("  {} ", l.time), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} ", l.icon), icon_style(l.icon)),
                Span::raw(&l.message),
            ])
        })
        .collect();

    let log_height = chunks[1].height.saturating_sub(2) as usize;
    let total_lines = detail_lines.len();
    let max_scroll = total_lines.saturating_sub(log_height) as u16;
    if app.detail_scroll > max_scroll {
        app.detail_scroll = max_scroll;
    }

    let detail_log = Paragraph::new(detail_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Events "),
        )
        .scroll((app.detail_scroll, 0));

    f.render_widget(detail_log, chunks[1]);

    if total_lines > log_height {
        let mut scrollbar_state = ScrollbarState::new(max_scroll as usize)
            .position(app.detail_scroll as usize);
        f.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            chunks[1],
            &mut scrollbar_state,
        );
    }

    // ── Help ──
    let help = Line::from(vec![
        Span::styled("  Esc", Style::default().fg(Color::White).bold()),
        Span::styled(" back  ", Style::default().fg(Color::DarkGray)),
        Span::styled("↑↓", Style::default().fg(Color::White).bold()),
        Span::styled(" scroll", Style::default().fg(Color::DarkGray)),
    ]);
    f.render_widget(Paragraph::new(help), chunks[2]);
}

fn icon_style(icon: &str) -> Style {
    match icon {
        "▶" => Style::default().fg(Color::Cyan),
        "$" => Style::default().fg(Color::Yellow),
        "✓" => Style::default().fg(Color::Green),
        "✗" => Style::default().fg(Color::Red),
        "⚙" => Style::default().fg(Color::Cyan),
        "→" | "←" => Style::default().fg(Color::DarkGray),
        "↔" => Style::default().fg(Color::DarkGray),
        _ => Style::default(),
    }
}
