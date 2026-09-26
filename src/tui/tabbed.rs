//! One agent's page: its live screen, its diff, or its log, under a row of
//! tabs, in the space the timeline otherwise has.

use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Tabs};

use super::text::{pane, wrap_lines};
use super::{TuiState, View};
use crate::event_log;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    /// The agent's terminal as tmux draws it, and typed at.
    Live,
    /// What its worktree changed since it started.
    Diff,
    /// Its transcript and the log's record of it.
    Log,
}

impl Tab {
    pub const ALL: [Tab; 3] = [Tab::Live, Tab::Diff, Tab::Log];

    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Diff => "diff",
            Self::Log => "log",
        }
    }

    /// The tab after this one among `tabs`, round again past the last.
    pub fn next_in(self, tabs: &[Tab]) -> Self {
        let at = tabs.iter().position(|tab| *tab == self).unwrap_or(0);
        tabs.get((at + 1) % tabs.len().max(1))
            .copied()
            .unwrap_or(self)
    }
}

pub fn draw_detail(frame: &mut Frame, area: Rect, state: &mut TuiState) {
    debug_assert_eq!(state.view(), View::Detail);
    let parts = Layout::vertical([Constraint::Length(1), Constraint::Min(3)]).split(area);
    let shown = state.tabs();
    let tabs = Tabs::new(shown.iter().map(|tab| format!(" {} ", tab.label())))
        .divider("│")
        .select(
            shown
                .iter()
                .position(|tab| *tab == state.tab())
                .unwrap_or(0),
        )
        .style(Style::new().fg(Color::DarkGray))
        .highlight_style(Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, parts[0]);
    match state.tab() {
        Tab::Live => draw_preview(frame, parts[1], state),
        Tab::Diff => draw_diff(frame, parts[1], state),
        Tab::Log => draw_log(frame, parts[1], state),
    }
}

fn title_of(state: &TuiState, what: &str) -> String {
    match state.selected_agent() {
        Some(agent) => format!(" {} · {what} ", agent.hash_id),
        None => format!(" {what} "),
    }
}

fn draw_preview(frame: &mut Frame, area: Rect, state: &mut TuiState) {
    let plain_title = title_of(state, "live");
    let block = pane(&plain_title, true);
    let inner = block.inner(area);
    state.preview_area = Some((inner.width, inner.height));
    state.preview_origin = Some((inner.x, inner.y));
    let Some(preview) = state.preview.as_mut() else {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                "This agent has no session to show.",
                Style::new().fg(Color::DarkGray),
            )))
            .block(block),
            area,
        );
        return;
    };
    let keys = preview.selecting_with_keys();
    let (title, bottom) = match preview.scroll_position() {
        Some((first, total)) => (
            title_of(
                state,
                if keys { "live · select" } else { "live · scroll" },
            ),
            format!(
                " {first}-{} of {total} ",
                (first + inner.height as usize).saturating_sub(1).min(total)
            ),
        ),
        None if state.typing() => (title_of(state, "live · typing"), String::new()),
        None => (title_of(state, "live"), String::new()),
    };
    let typing = state.typing();
    let preview = state.preview.as_mut().expect("checked above");
    let mut lines = preview.window(inner.height as usize);
    if let Some(error) = preview.error() {
        lines.push(Line::from(Span::styled(
            format!("tmux: {error}"),
            Style::new().fg(Color::Red),
        )));
    }
    // The agent's own cursor, so it is plain where the next key lands; while
    // selecting with the keys, theirs.
    let cursor = preview
        .cursor()
        .filter(|_| typing)
        .or(preview.select_cursor());
    let selection = preview.selection_range().zip(preview.offset());
    let block = pane(&title, true).title_bottom(Line::from(bottom).right_aligned());
    frame.render_widget(Paragraph::new(lines).block(block), area);
    // What is selected reads reversed, cell by cell, the way a terminal
    // shows its own selection.
    if let Some(((start, end), offset)) = selection {
        let buffer = frame.buffer_mut();
        for row in 0..inner.height {
            let line = offset + row as usize;
            if line < start.line || line > end.line {
                continue;
            }
            let from = if line == start.line { start.col } else { 0 };
            let to = if line == end.line {
                end.col + 1
            } else {
                inner.width as usize
            };
            for col in from..to.min(inner.width as usize) {
                if let Some(cell) = buffer.cell_mut((inner.x + col as u16, inner.y + row)) {
                    cell.set_style(Style::new().add_modifier(Modifier::REVERSED));
                }
            }
        }
    }
    if let Some((x, y)) = cursor {
        if x < inner.width && y < inner.height {
            frame.set_cursor_position((inner.x + x, inner.y + y));
        }
    }
}

fn draw_diff(frame: &mut Frame, area: Rect, state: &mut TuiState) {
    let viewport = area.height.saturating_sub(2) as usize;
    state.set_viewport(viewport);
    let lines: Vec<Line<'static>> = match state.diff.as_ref() {
        Some(report) => {
            let mut lines = vec![Line::from(vec![
                Span::styled(
                    format!("+{}", report.added),
                    Style::new().fg(Color::Green).add_modifier(Modifier::BOLD),
                ),
                Span::raw(" "),
                Span::styled(
                    format!("-{}", report.removed),
                    Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    match state.selected_agent().and_then(|agent| agent.base.clone()) {
                        Some(base) => {
                            format!("  since {}", base.chars().take(12).collect::<String>())
                        }
                        None => String::new(),
                    },
                    Style::new().fg(Color::DarkGray),
                ),
            ])];
            if report.text.trim().is_empty() {
                lines.push(Line::from(Span::styled(
                    "No changes in this worktree yet.",
                    Style::new().fg(Color::DarkGray),
                )));
            }
            for raw in report.text.lines() {
                let style = if raw.starts_with("+++") || raw.starts_with("---") {
                    Style::new().fg(Color::White).add_modifier(Modifier::BOLD)
                } else if raw.starts_with('+') {
                    Style::new().fg(Color::Green)
                } else if raw.starts_with('-') {
                    Style::new().fg(Color::Red)
                } else if raw.starts_with("@@") {
                    Style::new().fg(Color::Cyan)
                } else if raw.starts_with("diff --git") {
                    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::new()
                };
                lines.push(Line::from(Span::styled(raw.to_owned(), style)));
            }
            lines
        }
        None => vec![Line::from(Span::styled(
            match state
                .selected_agent()
                .and_then(|agent| agent.base.as_deref())
            {
                Some(_) => "Reading the diff…",
                None => "This agent's worktree has no recorded base to diff against.",
            },
            Style::new().fg(Color::DarkGray),
        ))],
    };
    state.set_detail_total(lines.len());
    let start = state.detail_start();
    let window: Vec<Line> = lines
        .into_iter()
        .skip(start)
        .take(viewport.max(1))
        .collect();
    let total = state.detail_total_rows();
    frame.render_widget(
        Paragraph::new(window).block(
            pane(&title_of(state, "diff"), true).title_bottom(
                Line::from(format!(
                    " {}-{} of {total} ",
                    if total == 0 { 0 } else { start + 1 },
                    (start + viewport.max(1)).min(total)
                ))
                .right_aligned(),
            ),
        ),
        area,
    );
}

/// The agent's transcript, as the messages page used to show it.
fn draw_log(frame: &mut Frame, area: Rect, state: &mut TuiState) {
    let viewport = area.height.saturating_sub(2) as usize;
    state.set_viewport(viewport);
    // Wrapping here rather than in the widget keeps the last line reachable:
    // the scroll offset and the rendered lines are then the same unit.
    let mut wrapped = wrap_lines(state.detail_body(), area.width.saturating_sub(2) as usize);
    let pulse = state.pulse_spans(event_log::now_ms() as i64);
    if !pulse.is_empty() {
        wrapped.push(Line::from(pulse));
    }
    state.set_detail_total(wrapped.len());
    let start = state.detail_start();
    let window: Vec<Line> = wrapped
        .into_iter()
        .skip(start)
        .take(viewport.max(1))
        .collect();
    let total = state.detail_total_rows();
    frame.render_widget(
        Paragraph::new(window).block(
            pane(&title_of(state, "log"), true).title_bottom(
                Line::from(format!(
                    " {}-{} of {total} ",
                    if total == 0 { 0 } else { start + 1 },
                    (start + viewport.max(1)).min(total)
                ))
                .right_aligned(),
            ),
        ),
        area,
    );
}
