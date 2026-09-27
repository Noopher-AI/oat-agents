// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

//! The live screen of one agent's tmux session, as the viewer shows it.
//!
//! tmux draws the screen; this only asks for it (`capture-pane -e`) and
//! turns the escape sequences into styled text. The pane is pinned to the
//! size of the box it is shown in, so what is captured is exactly what
//! would be seen sitting at that terminal. A hash of each capture tells
//! whether anything moved, which is how a row knows to spin.

use ansi_to_tui::IntoText;
use ratatui::prelude::*;
use sha2::{Digest, Sha256};
use std::time::Instant;
use unicode_width::UnicodeWidthChar;

use super::Control;

pub struct Preview {
    pub session: String,
    text: Text<'static>,
    last_hash: Option<[u8; 32]>,
    /// The size the pane was last told to be; `None` until it is known.
    pub sent_size: Option<(u16, u16)>,
    scroll: Option<ScrollMode>,
    /// When the screen last changed, for the spinner.
    pub changed_at: Option<Instant>,
    error: Option<String>,
    /// Where the pane's cursor stood at the last capture, when it shows one.
    cursor: Option<(u16, u16)>,
}

/// Reading back through what scrolled off: the whole history is taken
/// once, and the live refresh pauses until the viewer leaves.
struct ScrollMode {
    lines: Vec<Line<'static>>,
    offset: usize,
    viewport: usize,
    selection: Option<Selection>,
}

/// A place in the history: a line, and a screen column along it.
#[derive(Clone, Copy, Debug, Default, Eq, Ord, PartialEq, PartialOrd)]
pub struct Point {
    pub line: usize,
    pub col: usize,
}

/// Text being picked out of the history to copy. It runs from the anchor to
/// the cursor, both cells included, the way a terminal selects.
#[derive(Clone, Copy, Debug)]
struct Selection {
    /// Where it started; `None` while the keys are still placing the cursor.
    anchor: Option<Point>,
    cursor: Point,
    /// Made with the keys, which move the cursor, rather than with the mouse.
    keys: bool,
}

impl Preview {
    pub fn new(session: impl Into<String>) -> Self {
        Self {
            session: session.into(),
            text: Text::default(),
            last_hash: None,
            sent_size: None,
            scroll: None,
            changed_at: None,
            error: None,
            cursor: None,
        }
    }

    /// Takes a fresh capture; `true` when the screen differs from the last one.
    pub fn refresh(&mut self, control: &dyn Control) -> bool {
        match control.capture(&self.session) {
            Ok(raw) => {
                self.error = None;
                self.cursor = control.cursor(&self.session);
                let hash: [u8; 32] = Sha256::digest(raw.as_bytes()).into();
                if self.last_hash == Some(hash) {
                    return false;
                }
                self.last_hash = Some(hash);
                self.text = to_text(&raw);
                self.changed_at = Some(Instant::now());
                true
            }
            // The last frame stays; a session that went away says so once
            // under the box rather than blanking it.
            Err(error) => {
                self.error = Some(error.to_string());
                false
            }
        }
    }

    /// Pins the pane to the box, once per size.
    pub fn fit(&mut self, control: &dyn Control, cols: u16, rows: u16) {
        if cols < 2 || rows < 1 || self.sent_size == Some((cols, rows)) {
            return;
        }
        if control.resize(&self.session, cols, rows).is_ok() {
            self.sent_size = Some((cols, rows));
        }
    }

    /// Where the cursor is on the live screen; nothing while reading back.
    pub fn cursor(&self) -> Option<(u16, u16)> {
        if self.scroll.is_some() {
            return None;
        }
        self.cursor
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn scrolling(&self) -> bool {
        self.scroll.is_some()
    }

    /// Enters scroll mode at the bottom of the history.
    pub fn enter_scroll(&mut self, control: &dyn Control, viewport: usize) {
        if self.scroll.is_some() {
            return;
        }
        let raw = match control.capture_history(&self.session) {
            Ok(raw) => raw,
            Err(error) => {
                self.error = Some(error.to_string());
                return;
            }
        };
        let mut lines = to_text(&raw).lines;
        let viewport = viewport.max(1);
        // Both captures end on the same last line, but the live screen sits at the top of
        // the box and the history at its bottom. Blank lines under the history put its last
        // page where the live screen had it, so nothing moves on the way in and a row
        // pointed at on the live screen is the same line once reading back.
        let live = self.text.lines.len();
        let offset = if (1..=viewport).contains(&live) && live <= lines.len() {
            let offset = lines.len() - live;
            lines.resize(offset + viewport, Line::default());
            offset
        } else {
            lines.len().saturating_sub(viewport)
        };
        self.scroll = Some(ScrollMode {
            offset,
            lines,
            viewport,
            selection: None,
        });
    }

    /// Moves through the history; reaching the bottom does not leave scroll
    /// mode, because esc is the one key that means "back to live".
    pub fn scroll(&mut self, delta: isize) {
        let Some(scroll) = self.scroll.as_mut() else {
            return;
        };
        let last = scroll.lines.len().saturating_sub(scroll.viewport) as isize;
        scroll.offset = (scroll.offset as isize + delta).clamp(0, last.max(0)) as usize;
    }

    /// Whether reading back has reached the newest line.
    pub fn at_bottom(&self) -> bool {
        self.scroll
            .as_ref()
            .is_none_or(|scroll| scroll.offset + scroll.viewport >= scroll.lines.len())
    }

    pub fn leave_scroll(&mut self) {
        self.scroll = None;
    }

    /// Where the history view stands: first visible line and total, 1-based.
    pub fn scroll_position(&self) -> Option<(usize, usize)> {
        let scroll = self.scroll.as_ref()?;
        Some((
            if scroll.lines.is_empty() {
                0
            } else {
                scroll.offset + 1
            },
            scroll.lines.len(),
        ))
    }

    /// The history line shown on a row of the box, while reading back.
    pub fn point_at(&self, row: u16, col: u16) -> Option<Point> {
        let scroll = self.scroll.as_ref()?;
        let last = scroll.lines.len().checked_sub(1)?;
        Some(Point {
            line: (scroll.offset + row as usize).min(last),
            col: col as usize,
        })
    }

    /// Starts selecting with the keys: the cursor stands at the start of the
    /// newest line in view, and nothing is marked yet.
    pub fn begin_select(&mut self) {
        let Some(scroll) = self.scroll.as_mut() else {
            return;
        };
        let Some(last) = scroll.lines.len().checked_sub(1) else {
            return;
        };
        let bottom = (scroll.offset + scroll.viewport).min(scroll.lines.len());
        // The lowest line with something on it, not the blank rows under it.
        let line = (scroll.offset..bottom)
            .rev()
            .find(|&line| !plain(&scroll.lines[line]).trim().is_empty())
            .unwrap_or(bottom.saturating_sub(1))
            .min(last);
        scroll.selection = Some(Selection {
            anchor: None,
            cursor: Point { line, col: 0 },
            keys: true,
        });
    }

    /// A selection made with the mouse, from where the button went down.
    pub fn select_from(&mut self, at: Point) {
        if let Some(scroll) = self.scroll.as_mut() {
            scroll.selection = Some(Selection {
                anchor: Some(at),
                cursor: at,
                keys: false,
            });
        }
    }

    /// Moves the selection's moving end to `at`.
    pub fn select_to(&mut self, at: Point) {
        if let Some(selection) = self.scroll.as_mut().and_then(|scroll| scroll.selection.as_mut()) {
            selection.cursor = at;
        }
    }

    /// Whether the keys are moving a selection's cursor.
    pub fn selecting_with_keys(&self) -> bool {
        self.selection().is_some_and(|selection| selection.keys)
    }

    pub fn has_selection(&self) -> bool {
        self.selection().is_some()
    }

    fn selection(&self) -> Option<&Selection> {
        self.scroll.as_ref()?.selection.as_ref()
    }

    pub fn clear_selection(&mut self) {
        if let Some(scroll) = self.scroll.as_mut() {
            scroll.selection = None;
        }
    }

    /// Marks where the selection starts, or unmarks it.
    pub fn toggle_mark(&mut self) {
        if let Some(selection) = self.scroll.as_mut().and_then(|scroll| scroll.selection.as_mut()) {
            selection.anchor = match selection.anchor {
                Some(_) => None,
                None => Some(selection.cursor),
            };
        }
    }

    /// Moves the selection's cursor by lines and columns, keeping it in
    /// view. Across a line the cursor stops one past its last character.
    pub fn move_cursor(&mut self, lines: isize, cols: isize) {
        let Some(scroll) = self.scroll.as_mut() else {
            return;
        };
        let Some(last) = scroll.lines.len().checked_sub(1) else {
            return;
        };
        let Some(selection) = scroll.selection.as_mut() else {
            return;
        };
        let cursor = &mut selection.cursor;
        cursor.line = (cursor.line as isize + lines).clamp(0, last as isize) as usize;
        if cols != 0 {
            let end = display_width(&plain(&scroll.lines[cursor.line]));
            let from = cursor.col.min(end) as isize;
            cursor.col = (from + cols).clamp(0, end as isize) as usize;
        }
        let line = cursor.line;
        if line < scroll.offset {
            scroll.offset = line;
        } else if line >= scroll.offset + scroll.viewport {
            scroll.offset = line + 1 - scroll.viewport;
        }
    }

    /// Puts the cursor at the start (`false`) or the end (`true`) of its line.
    pub fn cursor_to_edge(&mut self, end: bool) {
        let Some(scroll) = self.scroll.as_mut() else {
            return;
        };
        let Some(selection) = scroll.selection.as_mut() else {
            return;
        };
        let cursor = &mut selection.cursor;
        cursor.col = if end {
            scroll
                .lines
                .get(cursor.line)
                .map(|line| display_width(&plain(line)).saturating_sub(1))
                .unwrap_or(0)
        } else {
            0
        };
    }

    /// The ends of the selection in reading order; the cursor alone while
    /// nothing is marked.
    pub fn selection_range(&self) -> Option<(Point, Point)> {
        let selection = self.selection()?;
        let anchor = selection.anchor.unwrap_or(selection.cursor);
        Some((anchor.min(selection.cursor), anchor.max(selection.cursor)))
    }

    /// Where the keys' cursor is in the box, when it is in view.
    pub fn select_cursor(&self) -> Option<(u16, u16)> {
        let scroll = self.scroll.as_ref()?;
        let selection = scroll.selection.as_ref().filter(|selection| selection.keys)?;
        let row = selection.cursor.line.checked_sub(scroll.offset)?;
        (row < scroll.viewport).then_some((selection.cursor.col as u16, row as u16))
    }

    /// The first line on screen while reading back.
    pub fn offset(&self) -> Option<usize> {
        self.scroll.as_ref().map(|scroll| scroll.offset)
    }

    /// What the selection holds, as text to copy: each line without the
    /// blanks that pad it out to the edge of the screen. With nothing
    /// marked, it is the cursor's whole line.
    pub fn selected_text(&self) -> Option<String> {
        let scroll = self.scroll.as_ref()?;
        let selection = scroll.selection.as_ref()?;
        let (start, end) = match selection.anchor {
            Some(anchor) => (anchor.min(selection.cursor), anchor.max(selection.cursor)),
            None => (
                Point { line: selection.cursor.line, col: 0 },
                Point { line: selection.cursor.line, col: usize::MAX - 1 },
            ),
        };
        let text = (start.line..=end.line)
            .filter_map(|line| scroll.lines.get(line))
            .enumerate()
            .map(|(at, line)| {
                let number = start.line + at;
                let from = if number == start.line { start.col } else { 0 };
                let to = (number == end.line).then_some(end.col + 1);
                columns(&plain(line), from, to).trim_end().to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        Some(text)
    }

    /// The lines to draw: the history window while scrolling, the live
    /// screen otherwise.
    pub fn window(&mut self, height: usize) -> Vec<Line<'static>> {
        match self.scroll.as_mut() {
            Some(scroll) => {
                scroll.viewport = height.max(1);
                let last = scroll.lines.len().saturating_sub(scroll.viewport);
                scroll.offset = scroll.offset.min(last);
                scroll
                    .lines
                    .iter()
                    .skip(scroll.offset)
                    .take(scroll.viewport)
                    .cloned()
                    .collect()
            }
            None => self.text.lines.iter().take(height).cloned().collect(),
        }
    }
}

/// A line's characters without their styles.
fn plain(line: &Line) -> String {
    line.spans.iter().map(|span| span.content.as_ref()).collect()
}

fn display_width(text: &str) -> usize {
    text.chars().map(|c| c.width().unwrap_or(0)).sum()
}

/// The characters of `text` that start in the screen columns `from..to`,
/// to the end of the line when `to` is `None`.
fn columns(text: &str, from: usize, to: Option<usize>) -> String {
    let mut col = 0;
    let mut out = String::new();
    for c in text.chars() {
        if col >= from && to.is_none_or(|to| col < to) {
            out.push(c);
        }
        col += c.width().unwrap_or(0);
    }
    out
}

/// Styled text from what tmux captured. Escapes it cannot read leave the
/// text plain rather than empty, and the blank tail a fixed-height pane
/// always carries is dropped so a short screen sits at the top.
pub fn to_text(raw: &str) -> Text<'static> {
    let mut text = raw.as_bytes().into_text().unwrap_or_else(|_| {
        Text::from(
            raw.lines()
                .map(|line| Line::raw(line.to_owned()))
                .collect::<Vec<_>>(),
        )
    });
    while text
        .lines
        .last()
        .is_some_and(|line| line.spans.iter().all(|span| span.content.trim().is_empty()))
    {
        text.lines.pop();
    }
    text
}
