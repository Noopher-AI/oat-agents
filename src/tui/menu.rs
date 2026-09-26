//! The line of key hints under every screen, and the notice line under it.
//!
//! The hints are grouped: keys that move, keys that act, keys that leave.
//! Groups are separated by a bar and items by a dot, so the line reads as
//! a few short phrases rather than one long one. When the terminal is too
//! narrow, items go from the back — the last one, the way out, always stays.

use ratatui::prelude::*;
use ratatui::widgets::Paragraph;
use std::time::{Duration, Instant};

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

/// One line of grouped hints, narrowed to fit rather than cut off.
pub fn line(groups: &[Vec<Item>], width: u16) -> Line<'static> {
    let mut keep: Vec<(usize, usize)> = groups
        .iter()
        .enumerate()
        .flat_map(|(g, items)| (0..items.len()).map(move |i| (g, i)))
        .collect();
    let measure = |keep: &[(usize, usize)]| -> usize {
        let mut width = 1;
        let mut last_group = None;
        for (g, i) in keep {
            let item = &groups[*g][*i];
            width += match last_group {
                None => 0,
                Some(previous) if previous == *g => 3,
                Some(_) => 3,
            };
            width += item.key.chars().count() + 1 + item.desc.chars().count();
            last_group = Some(*g);
        }
        width
    };
    while keep.len() > 1 && measure(&keep) > width as usize {
        keep.remove(keep.len() - 2);
    }
    let mut spans = vec![Span::raw(" ")];
    let mut last_group = None;
    for (g, i) in keep {
        let item = &groups[g][i];
        match last_group {
            None => {}
            Some(previous) if previous == g => {
                spans.push(Span::styled(" • ", Style::new().fg(Color::DarkGray)));
            }
            Some(_) => spans.push(Span::styled(" │ ", Style::new().fg(Color::DarkGray))),
        }
        spans.push(Span::styled(item.key, Style::new().fg(Color::Cyan)));
        spans.push(Span::raw(format!(" {}", item.desc)));
        last_group = Some(g);
    }
    Line::from(spans)
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
