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
        let lines = to_text(&raw).lines;
        let viewport = viewport.max(1);
        self.scroll = Some(ScrollMode {
            offset: lines.len().saturating_sub(viewport),
            lines,
            viewport,
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
