// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

//! The page under the roster: the selected agent's live screen, its diff, its timeline or
//! its log, under a row of tabs. With every agent selected, only the Run's timeline.

use ratatui::prelude::*;
use ratatui::widgets::{Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Tabs};

use super::TuiState;
use super::model::string_field;
use super::text::{clock, event_detail, event_glyph, pane, role_color, wrap_lines};
use crate::event_log;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Tab {
    /// The agent's terminal as tmux draws it, and typed at.
    Live,
    /// What its worktree changed since it started.
    Diff,
    /// The log's entries about it, one per line; every agent's when none is selected.
    Timeline,
    /// Its transcript and the log's record of it.
    Log,
}

impl Tab {
    pub const ALL: [Tab; 4] = [Tab::Live, Tab::Diff, Tab::Timeline, Tab::Log];

    pub fn label(self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::Diff => "diff",
            Self::Timeline => "timeline",
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

    /// The tab before this one among `tabs`, round again past the first.
    pub fn prev_in(self, tabs: &[Tab]) -> Self {
        let at = tabs.iter().position(|tab| *tab == self).unwrap_or(0);
        tabs.get((at + tabs.len().max(1) - 1) % tabs.len().max(1))
            .copied()
            .unwrap_or(self)
    }
}

pub fn draw_page(frame: &mut Frame, area: Rect, state: &mut TuiState) {
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
        Tab::Timeline => draw_timeline(frame, parts[1], state),
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
    // Grey while watching, lit only once enter has handed the agent the keys, so it is plain
    // at a glance whether a key goes to the view or to the agent.
    let block = pane(&plain_title, state.typing());
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
    let block = pane(&title, typing).title_bottom(Line::from(bottom).right_aligned());
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

/// The log's entries, one per line: the selected agent's, or the whole Run's.
fn draw_timeline(frame: &mut Frame, area: Rect, state: &mut TuiState) {
    state.set_viewport(area.height.saturating_sub(2) as usize);
    let column = state.hash_column();
    let timeline: Vec<Line> = state
        .window()
        .into_iter()
        .map(|row| {
            let event = string_field(row, "event").unwrap_or_default();
            let (glyph, glyph_color) = event_glyph(&event);
            let hash = string_field(row, "hash_id").unwrap_or_else(|| "·".to_owned());
            let role = state
                .agents
                .iter()
                .find(|agent| agent.hash_id == hash)
                .and_then(|agent| agent.role.clone());
            Line::from(vec![
                Span::styled(format!("{} ", clock(row)), Style::new().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:<width$}", hash, width = column),
                    Style::new().fg(role_color(role.as_deref())),
                ),
                Span::styled(format!("{glyph} "), Style::new().fg(glyph_color)),
                Span::styled(format!("{:<14}", event), Style::new().fg(glyph_color)),
                // Some event names fill their column; the detail still needs a gap after them.
                Span::raw(match event_detail(row) {
                    detail if event.chars().count() >= 14 && !detail.is_empty() => {
                        format!(" {detail}")
                    }
                    detail => detail,
                }),
            ])
        })
        .collect();
    let (first, total) = state.position();
    frame.render_widget(
        Paragraph::new(timeline).block(
            pane(&title_of(state, "timeline"), true).title_bottom(
                Line::from(format!(" {first}-{} of {total} ", state.last_row())).right_aligned(),
            ),
        ),
        area,
    );
    let mut scrollbar = ScrollbarState::new(total.saturating_sub(state.viewport_rows()))
        .position(first.saturating_sub(1));
    frame.render_stateful_widget(
        Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(None)
            .end_symbol(None),
        area.inner(Margin {
            vertical: 1,
            horizontal: 0,
        }),
        &mut scrollbar,
    );
}
