//! The Run picker: every Run the log knows about, and the keys that open
//! or close one.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::{List, ListItem, ListState, Paragraph};
use std::collections::HashMap;
use std::path::Path;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

use super::model::{self, RunSummary};
use super::text::{WAITING_MARK, local_stamp, pane};
use super::{Control, menu};
use crate::pricing::{self, Usage};

/// The Run picker: every Run the log knows about, wherever it was fired from.
pub struct RunList {
    runs: Vec<RunSummary>,
    cursor: usize,
}

/// The Run picker with its own state: which Run is about to be closed, and
/// what came of the last attempt.
pub struct Picker {
    pub list: RunList,
    pub(super) confirming: Option<String>,
    pub(super) notice: Option<String>,
}

impl Picker {
    pub fn new(runs: Vec<RunSummary>) -> Self {
        Self {
            list: RunList::new(runs),
            confirming: None,
            notice: None,
        }
    }

    pub fn refresh(&mut self, runs: Vec<RunSummary>) {
        self.list.refresh(runs);
        // A Run that finished on its own is no longer a question to answer.
        if let Some(run_id) = self.confirming.clone() {
            let still_running = self
                .list
                .runs()
                .iter()
                .any(|run| run.run_id == run_id && run.running());
            if !still_running {
                self.confirming = None;
            }
        }
    }

    /// The Run awaiting a yes, with how many agents the answer would stop.
    pub fn confirming(&self) -> Option<(&str, usize)> {
        let run_id = self.confirming.as_deref()?;
        let run = self.list.runs().iter().find(|run| run.run_id == run_id)?;
        Some((run_id, run.active))
    }

    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    pub fn on_key(&mut self, key: KeyEvent, control: &dyn Control) -> PickerAction {
        if key.kind == KeyEventKind::Release {
            return PickerAction::Continue;
        }
        // Closing a Run kills live agents, so it asks first and takes only a
        // deliberate yes; every other key means no.
        if let Some(run_id) = self.confirming.clone() {
            self.confirming = None;
            if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                self.notice = Some(match control.close_run(&run_id) {
                    Ok(summary) => format!("{run_id} {summary}"),
                    Err(error) => format!("{run_id} could not be closed: {error}"),
                });
            } else {
                self.notice = Some(format!("{run_id} left running"));
            }
            return PickerAction::Continue;
        }
        self.notice = None;
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => PickerAction::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                PickerAction::Quit
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.list.move_cursor(1);
                PickerAction::Continue
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.list.move_cursor(-1);
                PickerAction::Continue
            }
            KeyCode::Char('x') => {
                match self.list.selected() {
                    Some(run) if run.running() => self.confirming = Some(run.run_id.clone()),
                    Some(run) => self.notice = Some(format!("{} has nothing running", run.run_id)),
                    None => {}
                }
                PickerAction::Continue
            }
            // Cleaning takes nothing a person could still want: a dirty
            // worktree stays, and a Run with agents in it is refused.
            KeyCode::Char('c') => {
                match self.list.selected() {
                    Some(run) if run.running() => {
                        self.notice =
                            Some(format!("{} is still running; close it first", run.run_id))
                    }
                    Some(run) => {
                        let run_id = run.run_id.clone();
                        self.notice = Some(match control.clean_run(&run_id) {
                            Ok(summary) => format!("{run_id} {summary}"),
                            Err(error) => format!("{run_id} could not be cleaned: {error}"),
                        });
                    }
                    None => {}
                }
                PickerAction::Continue
            }
            KeyCode::Enter | KeyCode::Right => match self.list.selected() {
                Some(run) => PickerAction::Open(run.run_id.clone()),
                None => PickerAction::Continue,
            },
            _ => PickerAction::Continue,
        }
    }
}

impl RunList {
    pub fn new(mut runs: Vec<RunSummary>) -> Self {
        // Running Runs first, then the most recent history: an operator opens
        // this to watch something live far more often than to read an archive.
        runs.sort_by(|left, right| {
            right
                .running()
                .cmp(&left.running())
                .then_with(|| right.last.cmp(&left.last))
        });
        Self { runs, cursor: 0 }
    }

    pub fn runs(&self) -> &[RunSummary] {
        &self.runs
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn selected(&self) -> Option<&RunSummary> {
        self.runs.get(self.cursor)
    }

    /// Keeps the cursor on the same Run as the list is rebuilt underneath it.
    pub fn refresh(&mut self, runs: Vec<RunSummary>) {
        let current = self.selected().map(|run| run.run_id.clone());
        *self = Self::new(runs);
        if let Some(current) = current {
            if let Some(index) = self.runs.iter().position(|run| run.run_id == current) {
                self.cursor = index;
            }
        }
    }

    pub fn move_cursor(&mut self, delta: isize) {
        if self.runs.is_empty() {
            return;
        }
        let last = self.runs.len() as isize - 1;
        self.cursor = (self.cursor as isize + delta).clamp(0, last) as usize;
    }
}

/// What the picker decided to do with a key.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PickerAction {
    Continue,
    Open(String),
    Quit,
}

/// The screen an operator lands on: what is running, and what has run.
pub fn draw_runs(
    frame: &mut Frame,
    picker: &Picker,
    log_dir: Option<&Path>,
    spend: &HashMap<String, Usage>,
) {
    let list = &picker.list;
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(3),
        Constraint::Length(1),
    ])
    .split(frame.area());

    let running = list.runs().iter().filter(|run| run.running()).count();
    let mut everything = Usage::default();
    for usage in spend.values() {
        everything.merge(usage);
    }
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                " oat-agents ",
                Style::new().fg(Color::Black).bg(Color::Cyan),
            ),
            Span::raw(format!("  {} runs ({running} running)", list.runs().len())),
            Span::styled(
                match everything.cost() {
                    Some(cost) => format!(
                        "  {} tokens  {} total",
                        pricing::human_tokens(everything.tokens().total()),
                        pricing::human_money(cost)
                    ),
                    None => String::new(),
                },
                Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  times {}", model::local_offset())),
        ])),
        areas[0],
    );

    // Each column is as wide as its widest entry and the columns sit two
    // cells apart. The title follows the repo it belongs to and gives up
    // what the row cannot spare, so the columns after it stay on screen.
    let cells: Vec<RunCells> = list
        .runs()
        .iter()
        .map(|run| RunCells::of(run, spend))
        .collect();
    let width = |column: fn(&RunCells) -> &str| {
        cells
            .iter()
            .map(|row| column(row).width())
            .max()
            .unwrap_or(0)
    };
    let widths = [
        width(|row| &row.run_id),
        width(|row| &row.repo),
        width(|row| &row.title),
        width(|row| &row.time),
        width(|row| &row.span),
        width(|row| &row.agents),
        width(|row| &row.exec),
        width(|row| &row.cost),
    ];
    let [run_w, repo_w, title_w, time_w, span_w, agents_w, exec_w, cost_w] = widths;
    // Two border cells, the two-cell mark, and a gap after every column
    // but the last.
    let fixed =
        2 + 2 + run_w + repo_w + time_w + span_w + agents_w + exec_w + cost_w + GAP.len() * 7;
    let title_w = title_w.min((areas[1].width as usize).saturating_sub(fixed));

    let rows: Vec<ListItem> = list
        .runs()
        .iter()
        .zip(&cells)
        .map(|(run, row)| {
            // Waiting on a person outranks running or finished: it is the
            // only state in this list the reader can do something about.
            let (mark, mark_color) = match (run.question.is_some(), run.running()) {
                (true, _) => (WAITING_MARK, Color::Yellow),
                (false, true) => ("●", Color::Green),
                (false, false) => ("✓", Color::DarkGray),
            };
            let dim = Style::new().fg(Color::DarkGray);
            ListItem::new(Line::from(vec![
                Span::styled(format!("{mark} "), Style::new().fg(mark_color)),
                Span::styled(
                    pad_right(&row.run_id, run_w),
                    Style::new().fg(Color::White).add_modifier(Modifier::BOLD),
                ),
                Span::raw(GAP),
                Span::styled(pad_right(&row.repo, repo_w), Style::new().fg(Color::Cyan)),
                Span::raw(GAP),
                Span::styled(
                    pad_right(&clip(&row.title, title_w), title_w),
                    if run.question.is_some() {
                        Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                    } else {
                        Style::new()
                    },
                ),
                Span::raw(GAP),
                Span::styled(pad_right(&row.time, time_w), dim),
                Span::raw(GAP),
                Span::styled(
                    pad_left(&row.span, span_w),
                    if run.running() {
                        Style::new().fg(Color::Green)
                    } else {
                        dim
                    },
                ),
                Span::raw(GAP),
                Span::styled(pad_left(&row.agents, agents_w), dim),
                Span::raw(GAP),
                // Where its roles run their commands, said rather than left to be inferred.
                Span::styled(pad_right(&row.exec, exec_w), Style::new().fg(Color::Blue)),
                Span::raw(GAP),
                Span::styled(pad_left(&row.cost, cost_w), Style::new().fg(Color::Yellow)),
            ]))
        })
        .collect();
    let empty = rows.is_empty();
    let mut state = ListState::default().with_selected(Some(list.cursor()));
    frame.render_stateful_widget(
        List::new(if empty {
            vec![ListItem::new(Line::from(Span::styled(
                "No runs recorded yet. Start one with `oat-agents meta fire`.",
                Style::new().fg(Color::DarkGray),
            )))]
        } else {
            rows
        })
        .block(pane(
            &match log_dir {
                Some(dir) => format!(" runs · {} ", dir.display()),
                None => " runs ".to_owned(),
            },
            true,
        ))
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
        areas[1],
        &mut state,
    );

    if let Some((run_id, agents)) = picker.confirming() {
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    format!(" close {run_id}? "),
                    Style::new().fg(Color::Black).bg(Color::Yellow),
                ),
                Span::styled(
                    format!(
                        "  it stops {agents} live agent{}  ",
                        if agents == 1 { "" } else { "s" }
                    ),
                    Style::new().fg(Color::Yellow),
                ),
                Span::styled("y", Style::new().fg(Color::Cyan)),
                Span::raw(" confirm · any other key cancels"),
            ])),
            areas[2],
        );
        return;
    }
    if let Some(notice) = picker.notice() {
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                format!(" {notice}"),
                Style::new().fg(Color::Yellow),
            ))),
            areas[2],
        );
        return;
    }
    frame.render_widget(
        Paragraph::new(menu::line(
            &[
                vec![menu::item("↑↓", "run"), menu::item("enter", "open")],
                vec![
                    menu::item("x", "close"),
                    menu::item("c", "clean worktrees"),
                    menu::item("ctrl+\\", "console"),
                ],
                vec![menu::item("q", "quit")],
            ],
            areas[2].width,
        )),
        areas[2],
    );
}

/// The space between two columns of the Run list.
const GAP: &str = "  ";

/// What one row of the Run list says, before it is laid out.
struct RunCells {
    run_id: String,
    repo: String,
    title: String,
    time: String,
    span: String,
    agents: String,
    exec: String,
    cost: String,
}

impl RunCells {
    fn of(run: &RunSummary, spend: &HashMap<String, Usage>) -> Self {
        Self {
            run_id: run.run_id.clone(),
            repo: run.repo_name().unwrap_or("-").to_owned(),
            title: match &run.question {
                Some(question) => format!("{WAITING_MARK} {question}"),
                None => run
                    .objective
                    .clone()
                    .unwrap_or_else(|| "(no objective recorded)".to_owned()),
            },
            time: local_stamp(Some(&run.last), "%m-%d %H:%M"),
            span: run.seconds().map(model::human_span).unwrap_or_default(),
            agents: format!("{} agents", run.agents),
            exec: run.exec.clone(),
            cost: spend
                .get(&run.run_id)
                .and_then(|usage| usage.cost())
                .map(pricing::human_money)
                .unwrap_or_default(),
        }
    }
}

/// `text` cut to `width` terminal cells, ending in `…` when anything was cut.
fn clip(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_owned();
    }
    let mut kept = String::new();
    let mut used = 0;
    for ch in text.chars() {
        let cells = ch.width().unwrap_or(0);
        if used + cells + 1 > width {
            break;
        }
        kept.push(ch);
        used += cells;
    }
    if width > 0 {
        kept.push('…');
    }
    kept
}

fn pad_right(text: &str, width: usize) -> String {
    format!("{text}{}", " ".repeat(width.saturating_sub(text.width())))
}

fn pad_left(text: &str, width: usize) -> String {
    format!("{}{text}", " ".repeat(width.saturating_sub(text.width())))
}
