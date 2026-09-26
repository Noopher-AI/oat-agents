//! The key hints under every screen, and the notice line under them.
//!
//! Every key that does something where the view stands is listed here, and
//! only here: a pane's title says what state it is in, not which keys work.
//! The hints are grouped: keys that move, keys that act, keys that leave.
//! Groups are separated by a bar and items by a dot, so a row reads as a few
//! short phrases rather than one long one. They wrap onto up to three rows
//! no wider than `MAX_WIDTH`, a group moving down whole when it does not fit
//! the end of a row. Only when three rows cannot hold them do items go, from
//! the back — the last one, the way out, always stays.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::time::{Duration, Instant};

/// The widest a row of hints runs, however wide the terminal: past this
/// the eye has to travel too far to find a key.
pub const MAX_WIDTH: u16 = 100;
/// The most rows the hints take.
pub const MAX_ROWS: usize = 3;

pub struct Item {
    pub key: &'static str,
    pub desc: String,
}

pub fn item(key: &'static str, desc: impl Into<String>) -> Item {
    Item {
        key,
        desc: desc.into(),
    }
}

impl Item {
    fn width(&self) -> usize {
        self.key.chars().count() + 1 + self.desc.chars().count()
    }
}

/// The separator before an item, by whether it starts a new group; either
/// takes three cells.
const SAME_GROUP: &str = " • ";
const NEW_GROUP: &str = " │ ";
const GAP: usize = 3;

/// The hints laid out in rows: which items, by group and index, go on each.
fn layout(groups: &[Vec<Item>], keep: &[(usize, usize)], width: usize) -> Vec<Vec<(usize, usize)>> {
    let mut rows: Vec<Vec<(usize, usize)>> = vec![Vec::new()];
    // A row starts with one space of margin.
    let mut used = 1;
    let mut at = 0;
    while at < keep.len() {
        let (g, _) = keep[at];
        let group: Vec<(usize, usize)> = keep[at..]
            .iter()
            .take_while(|(other, _)| *other == g)
            .copied()
            .collect();
        let whole = group.iter().map(|(g, i)| groups[*g][*i].width()).sum::<usize>()
            + GAP * group.len().saturating_sub(1);
        let row_empty = rows.last().is_none_or(Vec::is_empty);
        // A group that would be split across rows moves down whole when it
        // fits a row of its own.
        if !row_empty && used + GAP + whole > width && whole < width {
            rows.push(Vec::new());
            used = 1;
        }
        for &(g, i) in &group {
            let row_empty = rows.last().is_none_or(Vec::is_empty);
            let gap = if row_empty { 0 } else { GAP };
            let item = groups[g][i].width();
            if !row_empty && used + gap + item > width {
                rows.push(Vec::new());
                used = 1;
            } else {
                used += gap;
            }
            used += item;
            rows.last_mut().expect("never empty").push((g, i));
        }
        at += group.len();
    }
    rows
}

/// The hints as rows of at most `MAX_WIDTH`, narrowed to fit rather than
/// cut off.
pub fn lines(groups: &[Vec<Item>], width: u16) -> Vec<Line<'static>> {
    let width = width.min(MAX_WIDTH) as usize;
    let mut keep: Vec<(usize, usize)> = groups
        .iter()
        .enumerate()
        .flat_map(|(g, items)| (0..items.len()).map(move |i| (g, i)))
        .collect();
    let mut rows = layout(groups, &keep, width);
    while keep.len() > 1 && rows.len() > MAX_ROWS {
        keep.remove(keep.len() - 2);
        rows = layout(groups, &keep, width);
    }
    rows.into_iter()
        .map(|row| {
            let mut spans = vec![Span::raw(" ")];
            let mut last_group = None;
            for (g, i) in row {
                let item = &groups[g][i];
                match last_group {
                    None => {}
                    Some(previous) if previous == g => {
                        spans.push(Span::styled(SAME_GROUP, Style::new().fg(Color::DarkGray)));
                    }
                    Some(_) => spans.push(Span::styled(NEW_GROUP, Style::new().fg(Color::DarkGray))),
                }
                spans.push(Span::styled(item.key, Style::new().fg(Color::Cyan)));
                spans.push(Span::raw(format!(" {}", item.desc)));
                last_group = Some(g);
            }
            Line::from(spans)
        })
        .collect()
}

/// What came of the last thing the operator did, shown for a few seconds
/// and then gone: a banner that stays is a banner nobody reads twice.
#[derive(Clone, Debug)]
pub struct Notice {
    text: String,
    since: Instant,
    ttl: Duration,
    /// Whether it is bad news, which reads in a louder colour.
    alarm: bool,
}

const NOTICE_TTL: Duration = Duration::from_secs(3);
const ALARM_TTL: Duration = Duration::from_secs(8);

impl Notice {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            since: Instant::now(),
            ttl: NOTICE_TTL,
            alarm: false,
        }
    }

    /// Bad news stays longer, because it is the one thing here the reader
    /// has to act on.
    pub fn alarm(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            since: Instant::now(),
            ttl: ALARM_TTL,
            alarm: true,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn visible_at(&self, now: Instant) -> bool {
        now.saturating_duration_since(self.since) < self.ttl
    }

    pub fn visible(&self) -> bool {
        self.visible_at(Instant::now())
    }
}

pub fn draw_notice(frame: &mut Frame, area: Rect, notice: Option<&Notice>) {
    let Some(notice) = notice.filter(|notice| notice.visible()) else {
        return;
    };
    // It is drawn over the key menu, which is longer: what the notice does
    // not cover would otherwise read as the end of its sentence.
    frame.render_widget(ratatui::widgets::Clear, area);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            format!(" {}", notice.text()),
            if notice.alarm {
                Style::new().fg(Color::Red).add_modifier(Modifier::BOLD)
            } else {
                Style::new().fg(Color::Yellow)
            },
        ))),
        area,
    );
}
