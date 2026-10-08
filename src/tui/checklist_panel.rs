// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

//! The checklist overlay: a read-only floating panel showing the open Run's
//! checklist, opened with Ctrl+L the way Ctrl+\\ opens the console.
//!
//! Unlike the console, this panel has nothing to type into and nothing to
//! launch — it only shows what `checklist update` last saved, re-read from
//! disk while it is open. It also only makes sense next to a Run, so unlike
//! the console it is wired up to open only when one is.
//!
//! An item linked to a Dispatch name carries that work's progress, read from the
//! Run store on the same poll, so the panel moves as the work does, not only when
//! the meta-agent remembers to check something off.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph};

use crate::checklist::{ChecklistStore, ItemProgress};
use crate::store::Store;
use crate::tui::model::{self, ChecklistRow};

pub struct ChecklistPanel {
    open: bool,
    run_id: Option<String>,
    checklist: Option<Vec<ChecklistRow>>,
}

impl ChecklistPanel {
    pub fn new() -> Self {
        Self {
            open: false,
            run_id: None,
            checklist: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn open_for(&mut self, run_id: String) {
        self.run_id = Some(run_id);
        self.open = true;
    }

    pub fn close(&mut self) {
        self.open = false;
    }

    /// Re-reads the checklist, and its linked items' Dispatches, from disk. No
    /// subprocess: the view polls this every second while the window is open,
    /// same as the console.
    pub fn refresh(&mut self, checklists: Option<&ChecklistStore>, store: Option<&Store>) {
        let (Some(checklists), Some(run_id)) = (checklists, self.run_id.as_deref()) else {
            self.checklist = None;
            return;
        };
        self.checklist = model::checklist_items(Some(checklists), store, run_id);
    }
}

impl Default for ChecklistPanel {
    fn default() -> Self {
        Self::new()
    }
}

/// Ctrl+L is unclaimed by every screen here and is not one of the
/// terminal's own flow-control keys. It is taken before the live tab hands
/// keys to an agent, so it opens the checklist even while typing.
pub fn is_toggle(key: &KeyEvent) -> bool {
    if key.kind == KeyEventKind::Release {
        return false;
    }
    matches!(key.code, KeyCode::Char('l') if key.modifiers.contains(KeyModifiers::CONTROL))
}

/// Where the window sits: centred, large enough to hold a checklist, never so
/// large that it hides the whole screen it was opened over. Same shape as the
/// console's overlay.
fn area(full: Rect) -> Rect {
    let width = (full.width * 3 / 5).clamp(50.min(full.width), 120.min(full.width));
    let height = (full.height * 3 / 5).clamp(10.min(full.height), 40.min(full.height));
    Rect {
        x: full.x + (full.width.saturating_sub(width)) / 2,
        y: full.y + (full.height.saturating_sub(height)) / 2,
        width,
        height,
    }
}

pub fn draw(frame: &mut Frame, panel: &ChecklistPanel) {
    let rect = area(frame.area());
    frame.render_widget(Clear, rect);

    let title = match &panel.checklist {
        Some(checklist) => {
            let done = checklist.iter().filter(|row| row.done).count();
            format!(" checklist · {done}/{} ", checklist.len())
        }
        None => " checklist ".to_owned(),
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(Color::Cyan))
        .title(Span::styled(
            title,
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let lines: Vec<Line> = match &panel.checklist {
        None => vec![Line::from(Span::styled(
            "No checklist recorded for this run.",
            Style::new().fg(Color::DarkGray),
        ))],
        Some(checklist) if checklist.is_empty() => vec![Line::from(Span::styled(
            "No checklist recorded for this run.",
            Style::new().fg(Color::DarkGray),
        ))],
        Some(checklist) => checklist
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let mark = if row.done { "[x]" } else { "[ ]" };
                let style = if row.done {
                    Style::new().fg(Color::DarkGray)
                } else {
                    Style::new()
                };
                let mut spans = vec![Span::styled(
                    format!("{mark} {}. {}", index + 1, row.text),
                    style,
                )];
                if let Some(progress) = &row.progress {
                    // Finished and released but never checked: most likely forgotten.
                    if !row.done && progress.looks_done() {
                        spans.push(Span::styled(
                            " ?",
                            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                        ));
                    }
                    spans.push(Span::styled(
                        format!(" · {}", progress.summary()),
                        progress_style(row.done, progress),
                    ));
                }
                Line::from(spans)
            })
            .collect(),
    };
    frame.render_widget(
        Paragraph::new(lines).wrap(ratatui::widgets::Wrap { trim: false }),
        inner,
    );
}

/// Running work stands out; everything else stays out of the way of the items themselves.
fn progress_style(done: bool, progress: &ItemProgress) -> Style {
    match progress {
        ItemProgress::Active { .. } if !done => Style::new().fg(Color::Green),
        ItemProgress::Queued { .. } if !done => Style::new().fg(Color::Yellow),
        _ => Style::new().fg(Color::DarkGray),
    }
}
