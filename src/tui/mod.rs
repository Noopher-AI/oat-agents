//! The live view: every Run on this machine, each Run's roster and timeline, and each
//! Dispatch's live screen, diff and log.
//!
//! The Store and the workflow log are what the roster and the timeline are built from —
//! files an observer may always read; the agents themselves are watched through their tmux
//! sessions, which is also how a keystroke reaches one, and only through `Control`. All
//! state transitions are pure so they can be tested without a terminal.

use anyhow::{Context, Result};
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::crossterm::event::{
    DisableBracketedPaste, EnableBracketedPaste, KeyboardEnhancementFlags,
    PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::style::Print;
use ratatui::crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
    supports_keyboard_enhancement,
};
use ratatui::prelude::*;
use ratatui::widgets::{
    List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Stdout};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::checklist::ChecklistStore;
use crate::environment::Environment;
use crate::event_log::{self, EventLog, events};
use crate::pricing::{self, Usage};
use crate::store::Store;
use crate::transcript;

pub mod checklist_panel;
pub mod control;
pub mod live;
pub mod menu;
pub mod model;
pub mod picker;
pub mod preview;
pub mod tabbed;
pub mod text;

use checklist_panel::ChecklistPanel;
pub use control::{ConsoleHandle, Control, DiffReport, RealControl};
use model::{AgentRow, RunSnapshot, string_field};
pub use picker::{Picker, PickerAction, draw_runs};
pub use tabbed::Tab;

use live::LiveInput;
use menu::Notice;
use preview::Preview;
use text::*;

/// How often the log and the roster are re-read.
const POLL_INTERVAL: Duration = Duration::from_millis(1000);
/// How often a live screen is re-captured while it is being looked at.
const PREVIEW_INTERVAL: Duration = Duration::from_millis(100);
const INPUT_TIMEOUT: Duration = Duration::from_millis(200);
/// How long after its screen last moved an agent still reads as busy.
const BUSY_FOR: Duration = Duration::from_millis(2000);
/// How many live sessions the roster captures each poll for its spinners;
/// past that it only asks whether they exist.
const SPINNER_ROWS: usize = 12;
/// Every fifth poll the roster re-counts each live worktree's diff.
const DIFF_EVERY: u64 = 5;

/// Which events the timeline shows.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EventFilter {
    All,
    Lifecycle,
    Decisions,
}

impl EventFilter {
    fn label(self) -> &'static str {
        match self {
            Self::All => "all",
            Self::Lifecycle => "lifecycle",
            Self::Decisions => "decisions",
        }
    }

    fn next(self) -> Self {
        match self {
            Self::All => Self::Lifecycle,
            Self::Lifecycle => Self::Decisions,
            Self::Decisions => Self::All,
        }
    }

    fn accepts(self, event: &str) -> bool {
        match self {
            Self::All => true,
            Self::Lifecycle => matches!(event, events::AGENT_ENTER | events::AGENT_EXIT),
            Self::Decisions => matches!(
                event,
                events::DECISION
                    | events::DELEGATION
                    | events::REVIEW_RESULT
                    | events::RETRY
                    | events::BLOCKER
                    | events::NOTE
                    | events::NEEDS_HUMAN
                    | events::HUMAN_ANSWER
            ),
        }
    }
}

/// Which pane the arrow keys drive.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Focus {
    Agents,
    Timeline,
}

impl Focus {
    fn other(self) -> Self {
        match self {
            Self::Agents => Self::Timeline,
            Self::Timeline => Self::Agents,
        }
    }
}

/// What each transcript says, remembered so a large session file is parsed once rather
/// than on every poll. A file is re-read only when its size or modification time changes.
#[derive(Default)]
pub struct SpendIndex {
    seen: HashMap<PathBuf, (u64, i64, transcript::Session)>,
}

impl SpendIndex {
    pub fn of_file(&mut self, path: &Path) -> transcript::Session {
        let stamp = fs::metadata(path)
            .map(|meta| {
                (
                    meta.len(),
                    meta.modified()
                        .ok()
                        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|since| since.as_secs() as i64)
                        .unwrap_or_default(),
                )
            })
            .unwrap_or_default();
        if let Some((len, modified, session)) = self.seen.get(path) {
            if (*len, *modified) == stamp {
                return session.clone();
            }
        }
        let session = transcript::session(path);
        self.seen
            .insert(path.to_owned(), (stamp.0, stamp.1, session.clone()));
        session
    }

    /// What one Dispatch has spent, from the newest transcript its backend keeps for its
    /// worktree.
    pub fn of_agent(
        &mut self,
        projects_root: Option<&Path>,
        agent: &AgentRow,
    ) -> transcript::Session {
        let found = projects_root.zip(agent.worktree.as_deref()).and_then(|(root, worktree)| {
            transcript::find_transcript(root, worktree)
        });
        match found {
            Some(path) => self.of_file(&path),
            None => transcript::Session::default(),
        }
    }

    /// What a whole Run has spent.
    pub fn of_run(&mut self, projects_root: Option<&Path>, agents: &[AgentRow]) -> Usage {
        let mut total = Usage::default();
        for agent in agents {
            total.merge(&self.of_agent(projects_root, agent).usage);
        }
        total
    }
}

/// Which screen is on top.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum View {
    /// Agent list beside the timeline.
    Overview,
    /// One agent's own page: its screen, its diff, its log.
    Detail,
}

/// What the loop should do after a key press.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Action {
    Continue,
    /// Leave the open Run and go back to the picker.
    Back,
    Quit,
    /// End every live agent of this Run.
    CloseRun(String),
    /// End the console's session.
    StopConsole,
}

/// What the view knows about one agent's session, refreshed with the roster.
#[derive(Clone, Debug, Default)]
pub struct SessionInfo {
    pub alive: bool,
    last_hash: Option<[u8; 32]>,
    pub changed_at: Option<Instant>,
    pub diff_stat: Option<(u32, u32)>,
}

pub struct TuiState {
    pub(crate) run_id: String,
    /// The repository the Run was fired against, whose console `ctrl+\\` opens; for the
    /// console's own window, the repository it belongs to.
    pub(crate) repo: Option<PathBuf>,
    pub(crate) backend: String,
    events: Vec<Value>,
    agents: Vec<AgentRow>,
    /// 0 selects every agent; 1..=agents.len() selects one.
    pub(super) selected: usize,
    filter: EventFilter,
    follow: bool,
    /// Index of the first visible timeline row when not following.
    scroll: usize,
    viewport: usize,
    focus: Focus,
    view: View,
    tab: Tab,
    /// First visible wrapped line of the detail screen.
    detail_scroll: usize,
    /// Wrapped line count of the detail screen, known once it is drawn.
    detail_total: usize,
    /// Whether the detail screen stays pinned to the newest turn.
    detail_follow: bool,
    /// Where Claude Code keeps its session files, when it is reachable.
    projects_root: Option<PathBuf>,
    /// What each agent runs as and has spent, refreshed with the view.
    spend: HashMap<String, transcript::Session>,
    /// The selected agent's own turns, reloaded while the detail screen is up.
    transcript: Vec<transcript::Row>,
    transcript_source: Option<PathBuf>,
    /// The message being written, which outlives the box it is written
    /// in: esc puts the draft down, and `N` picks it back up.
    notice: Option<Notice>,
    /// The selected agent's live screen while its page is open.
    pub(super) preview: Option<Preview>,
    /// The size the preview box was last drawn at, so the pane can follow.
    pub(super) preview_area: Option<(u16, u16)>,
    /// Where on the terminal that box's top-left cell was, for the mouse.
    pub(super) preview_origin: Option<(u16, u16)>,
    /// Where in the box the left button went down, until it comes up.
    pressed: Option<(u16, u16)>,
    /// The button moved while down, so a selection is being made.
    dragged: bool,
    pub(super) diff: Option<DiffReport>,
    sessions: HashMap<String, SessionInfo>,
    /// The Run the operator is about to close, awaiting a yes.
    closing: bool,
    tick: u64,
    /// This is the console's window rather than a Run's: the console
    /// belongs to no Run (ADR-0005), so there is no Run here to close or to read.
    pub(crate) console: bool,
    /// Keys on the live tab go to the agent rather than to this view, until
    /// ctrl+] takes them back.
    typing: bool,
    /// Typing failed because the session is gone. The console, which is
    /// meant to always be there, is started again when this is set.
    pub(crate) session_lost: bool,
}

impl TuiState {
    pub fn with_projects(snapshot: RunSnapshot, projects_root: Option<PathBuf>) -> Self {
        let mut state = Self::empty(snapshot.run.id.clone(), projects_root);
        state.repo = Some(PathBuf::from(&snapshot.run.repo)).filter(|repo| !repo.as_os_str().is_empty());
        state.backend = snapshot.run.backend.clone();
        state.refresh(snapshot);
        state
    }

    fn empty(run_id: String, projects_root: Option<PathBuf>) -> Self {
        Self {
            run_id,
            repo: None,
            backend: String::new(),
            events: Vec::new(),
            agents: Vec::new(),
            selected: 0,
            filter: EventFilter::All,
            follow: true,
            scroll: 0,
            viewport: 10,
            focus: Focus::Agents,
            view: View::Overview,
            tab: Tab::Live,
            detail_scroll: 0,
            detail_total: 0,
            detail_follow: true,
            projects_root,
            spend: HashMap::new(),
            transcript: Vec::new(),
            transcript_source: None,
            notice: None,
            preview: None,
            preview_area: None,
            preview_origin: None,
            pressed: None,
            dragged: false,
            diff: None,
            sessions: HashMap::new(),
            closing: false,
            tick: 0,
            console: false,
            typing: false,
            session_lost: false,
        }
    }

    /// Reads each agent's spend through the shared index.
    pub fn refresh_spend(&mut self, usage: &mut SpendIndex) {
        let root = self.projects_root.clone();
        for agent in &self.agents {
            self.spend.insert(
                agent.hash_id.clone(),
                usage.of_agent(root.as_deref(), agent),
            );
        }
    }

    /// The question this Run is waiting on a person to answer.
    pub fn open_question(&self) -> Option<String> {
        model::open_question(&self.events)
    }

    pub fn spend_of(&self, hash_id: &str) -> Option<&transcript::Session> {
        self.spend
            .get(hash_id)
            .filter(|session| !session.usage.is_empty())
    }

    /// What the open Run has spent in total.
    pub fn spend_total(&self) -> Usage {
        let mut total = Usage::default();
        for agent in &self.agents {
            if let Some(session) = self.spend.get(&agent.hash_id) {
                total.merge(&session.usage);
            }
        }
        total
    }

    /// Re-reads the Run, keeping the selected agent if it still exists.
    pub fn refresh(&mut self, snapshot: RunSnapshot) {
        let agents = snapshot.agents();
        self.refresh_rows(agents, snapshot.events);
    }

    /// Replaces the roster and the event list, keeping the selected agent if it still exists.
    fn refresh_rows(&mut self, agents: Vec<AgentRow>, events: Vec<Value>) {
        let previous = self.selected_agent().map(|agent| agent.hash_id.clone());
        self.agents = agents;
        // Whoever is still running is what an operator is watching, so the
        // live agents stay on top in the order they entered. Below them the
        // finished ones read newest first, because a Run's tail is what a
        // reader is usually catching up on.
        self.agents.sort_by(|left, right| {
            let finished = |agent: &AgentRow| agent.state() == "exited";
            finished(left).cmp(&finished(right)).then_with(|| {
                if finished(left) && finished(right) {
                    last_seen(right).cmp(&last_seen(left))
                } else {
                    std::cmp::Ordering::Equal
                }
            })
        });
        self.events = events;
        self.selected = match previous {
            Some(hash) => self
                .agents
                .iter()
                .position(|agent| agent.hash_id == hash)
                .map(|index| index + 1)
                .unwrap_or(0),
            None => 0,
        };
        self.clamp_scroll();
        if self.view == View::Detail {
            // An agent that finishes while its page is open is left with
            // only its log, as if it had been opened finished.
            if self.tabs() == [Tab::Log] && self.tab != Tab::Log {
                self.tab = Tab::Log;
                self.typing = false;
                self.preview = None;
                self.diff = None;
            }
            self.load_transcript();
        }
    }

    /// Asks each live session whether it exists and whether its screen has
    /// moved, and every few polls what its worktree has changed. Cheap per
    /// row, and bounded: a roster of dozens only gets the existence check.
    pub fn refresh_sessions(&mut self, control: &dyn Control) {
        self.tick += 1;
        let count_diffs = self.tick % DIFF_EVERY == 1;
        let live: Vec<AgentRow> = self
            .agents
            .iter()
            .filter(|agent| agent.state() == "active")
            .cloned()
            .collect();
        for (index, agent) in live.iter().enumerate() {
            let Some(session) = agent.terminal_handle.as_deref() else {
                continue;
            };
            let info = self.sessions.entry(agent.hash_id.clone()).or_default();
            info.alive = control.is_alive(session);
            if info.alive && index < SPINNER_ROWS {
                if let Ok(raw) = control.capture(session) {
                    let hash: [u8; 32] = Sha256::digest(raw.as_bytes()).into();
                    if info.last_hash != Some(hash) {
                        info.last_hash = Some(hash);
                        info.changed_at = Some(Instant::now());
                    }
                }
            }
            if count_diffs {
                if let (Some(worktree), Some(base)) = (agent.worktree.as_deref(), agent.base.as_deref())
                {
                    info.diff_stat = control.diff_stat(Path::new(worktree), base).ok();
                }
            }
        }
        if self.view == View::Detail && self.tab == Tab::Diff {
            self.refresh_diff(control);
        }
    }

    fn refresh_diff(&mut self, control: &dyn Control) {
        let Some(agent) = self.selected_agent() else {
            return;
        };
        let (Some(worktree), Some(base)) = (agent.worktree.clone(), agent.base.clone()) else {
            self.diff = None;
            return;
        };
        if let Ok(report) = control.diff(Path::new(&worktree), &base) {
            self.diff = Some(report);
        }
    }

    /// Re-captures the open agent's screen, unless the viewer is reading
    /// back through its history.
    pub fn refresh_preview(&mut self, control: &dyn Control) {
        if self.view != View::Detail || self.tab != Tab::Live {
            return;
        }
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        if preview.scrolling() {
            return;
        }
        if preview.refresh(control) {
            if let Some(agent) = self.selected_agent() {
                let hash = agent.hash_id.clone();
                self.sessions.entry(hash).or_default().changed_at = Some(Instant::now());
            }
        }
    }

    /// Pins the pane to the box it was last drawn in.
    pub fn fit_preview(&mut self, control: &dyn Control) {
        let (Some(preview), Some((cols, rows))) = (self.preview.as_mut(), self.preview_area) else {
            return;
        };
        preview.fit(control, cols, rows);
    }

    /// Whether the live screen is what is being looked at right now.
    pub fn preview_visible(&self) -> bool {
        self.view == View::Detail
            && self.tab == Tab::Live
            && self
                .preview
                .as_ref()
                .is_some_and(|preview| !preview.scrolling())
    }

    /// Reads the selected agent's own turns from the backend session files.
    fn load_transcript(&mut self) {
        self.transcript.clear();
        self.transcript_source = None;
        let (Some(root), Some(agent)) = (self.projects_root.clone(), self.selected_agent()) else {
            return;
        };
        let Some(worktree) = agent.worktree.clone() else {
            return;
        };
        if let Some(path) = transcript::find_transcript(&root, &worktree) {
            self.transcript = transcript::render(&path, 400);
            self.transcript_source = Some(path);
        }
    }

    /// The open agent's live signal, for the log page.
    pub fn pulse_spans(&self, now_ms: i64) -> Vec<Span<'static>> {
        let Some(agent) = self.selected_agent() else {
            return Vec::new();
        };
        pulse_spans(
            self.spend.get(&agent.hash_id),
            agent.state() == "active",
            now_ms,
        )
    }

    pub fn selected_agent(&self) -> Option<&AgentRow> {
        self.selected
            .checked_sub(1)
            .and_then(|index| self.agents.get(index))
    }

    pub fn agents(&self) -> &[AgentRow] {
        &self.agents
    }

    /// The longest identifier plus a gap: without the gap the longest name
    /// runs straight into the next column with nothing between them.
    pub fn hash_column(&self) -> usize {
        self.agents
            .iter()
            .map(|agent| agent.hash_id.chars().count())
            .max()
            .unwrap_or(12)
            .saturating_add(2)
            .clamp(14, 46)
    }

    pub fn filter(&self) -> EventFilter {
        self.filter
    }

    pub fn following(&self) -> bool {
        self.follow
    }

    pub fn focus(&self) -> Focus {
        self.focus
    }

    pub fn view(&self) -> View {
        self.view
    }

    pub fn tab(&self) -> Tab {
        self.tab
    }

    #[cfg(test)]
    pub fn notice(&self) -> Option<&str> {
        self.notice
            .as_ref()
            .filter(|notice| notice.visible())
            .map(Notice::text)
    }

    pub fn set_notice(&mut self, notice: Notice) {
        self.notice = Some(notice);
    }

    /// Whether keys on the live tab are going to the agent.
    pub fn typing(&self) -> bool {
        self.typing
    }

    #[cfg(test)]
    pub fn closing(&self) -> bool {
        self.closing
    }

    /// First visible wrapped line, pinned to the newest turn while following.
    pub fn detail_start(&self) -> usize {
        let last_page = self.detail_total.saturating_sub(self.viewport);
        if self.detail_follow {
            last_page
        } else {
            self.detail_scroll.min(last_page)
        }
    }

    #[cfg(test)]
    pub fn detail_following(&self) -> bool {
        self.detail_follow
    }

    pub fn detail_total_rows(&self) -> usize {
        self.detail_total
    }

    /// What the roster shows beside an agent: a spinner while its screen
    /// moves, a dot while it sits, a pause mark when its session is gone,
    /// and a tick or a cross once the log has seen it exit.
    pub fn status_glyph(
        &self,
        agent: &AgentRow,
        now: Instant,
        now_ms: i64,
    ) -> (&'static str, Color) {
        if agent.state() == "exited" {
            return match agent.outcome.as_deref() {
                Some(outcome) if outcome.contains("fail") || outcome.contains("error") => {
                    ("✗", Color::Red)
                }
                _ => ("✓", Color::DarkGray),
            };
        }
        match self.sessions.get(&agent.hash_id) {
            Some(info) if !info.alive => ("⏸", Color::Yellow),
            Some(info)
                if info
                    .changed_at
                    .is_some_and(|at| now.saturating_duration_since(at) < BUSY_FOR) =>
            {
                (spinner_frame(now_ms), Color::Green)
            }
            _ => ("●", Color::Green),
        }
    }

    pub fn diff_stat_of(&self, hash_id: &str) -> Option<(u32, u32)> {
        self.sessions.get(hash_id).and_then(|info| info.diff_stat)
    }

    /// The tabs the open agent's page has. A finished agent has only its
    /// log; one still running has its terminal and its diff as well.
    pub fn tabs(&self) -> &'static [Tab] {
        if self
            .selected_agent()
            .is_some_and(|agent| agent.state() == "exited")
        {
            &[Tab::Log]
        } else if self.console {
            // It stands in its own directory, not in a worktree: there is
            // no diff to show.
            &[Tab::Live, Tab::Log]
        } else {
            &Tab::ALL
        }
    }

    /// The session the live tab is showing, when it is a running one.
    fn live_session(&self) -> Option<String> {
        if self.view != View::Detail || self.tab != Tab::Live {
            return None;
        }
        self.preview.as_ref().map(|preview| preview.session.clone())
    }

    /// Hands a key or a paste to the live session, and shows what it did
    /// straight away rather than on the next capture.
    fn type_live(&mut self, session: &str, input: LiveInput, control: &dyn Control) {
        if let Some(preview) = self.preview.as_mut() {
            preview.leave_scroll();
        }
        match control.type_input(session, &input) {
            Ok(()) => {
                if let Some(preview) = self.preview.as_mut() {
                    preview.refresh(control);
                }
            }
            Err(error) => {
                if control.is_alive(session) {
                    self.notice = Some(Notice::alarm(format!("typing failed: {error}")));
                } else {
                    self.typing = false;
                    self.session_lost = true;
                    self.notice = Some(Notice::alarm("this agent's session is gone"));
                }
            }
        }
    }

    /// Starts typing when the live tab shows a running session.
    fn start_typing(&mut self) {
        self.typing = self.live_session().is_some();
    }

    /// Recorded when the screen is drawn, because wrapping depends on width.
    pub fn set_detail_total(&mut self, total: usize) {
        self.detail_total = total;
    }

    /// Moves the detail screen by whole wrapped lines. Reaching the last line
    /// rejoins follow, so a live agent keeps streaming.
    fn scroll_detail(&mut self, delta: isize) {
        let last_page = self.detail_total.saturating_sub(self.viewport) as isize;
        let next = (self.detail_start() as isize + delta).clamp(0, last_page.max(0));
        self.detail_scroll = next as usize;
        self.detail_follow = next >= last_page;
    }

    /// The log page: who the agent is, then its own turns inside the
    /// backend session, falling back to the log when no transcript is found.
    pub fn detail_body(&self) -> Vec<Line<'static>> {
        let mut lines = self.detail_lines();
        if self.transcript.is_empty() {
            lines.push(Line::from(Span::styled(
                match self.projects_root {
                    Some(_) => "No transcript found for this worktree yet.",
                    None => "Claude Code's session directory is not reachable.",
                },
                Style::new().fg(Color::DarkGray),
            )));
            lines.push(Line::raw(""));
            lines.extend(self.log_lines());
            return lines;
        }
        lines.extend(transcript_lines(&self.transcript));
        lines
    }

    /// Everything the log holds about the selected agent, in full.
    pub fn detail_lines(&self) -> Vec<Line<'static>> {
        let Some(agent) = self.selected_agent() else {
            return Vec::new();
        };
        let color = role_color(agent.role.as_deref());
        let mut lines = vec![
            Line::from(vec![
                Span::styled(
                    agent.hash_id.clone(),
                    Style::new().fg(color).add_modifier(Modifier::BOLD),
                ),
                Span::raw(format!(
                    "  {}  {}",
                    agent.role.clone().unwrap_or_else(|| "-".into()),
                    agent.agent.clone().unwrap_or_default()
                )),
            ]),
            Line::from(Span::styled(
                format!(
                    "dispatch {}   worktree {}{}",
                    agent.dispatch_id.clone().unwrap_or_else(|| "-".into()),
                    agent.worktree.clone().unwrap_or_else(|| "-".into()),
                    agent
                        .branch
                        .as_deref()
                        .map(|branch| format!(" ({branch})"))
                        .unwrap_or_default()
                ),
                Style::new().fg(Color::DarkGray),
            )),
            Line::from(Span::styled(
                format!(
                    "{}   entered {}   exited {}{}",
                    agent.state(),
                    local_stamp(agent.entered.as_deref(), "%m-%d %H:%M:%S"),
                    local_stamp(agent.exited.as_deref(), "%m-%d %H:%M:%S"),
                    agent
                        .outcome
                        .as_ref()
                        .map(|outcome| format!("   outcome {outcome}"))
                        .unwrap_or_default()
                ),
                Style::new().fg(Color::DarkGray),
            )),
            Line::raw(""),
        ];

        // Where this agent's commands ran, and only when that is somewhere other than the
        // host — or when it should have been and was not, which is never left to be inferred.
        match (agent.env_id.as_deref(), agent.env_skipped.as_deref()) {
            (Some(env_id), _) => lines.insert(
                2,
                Line::from(Span::styled(
                    format!(
                        "env pod:{env_id}   image {}",
                        agent
                            .image_id
                            .as_deref()
                            .map(short_image)
                            .unwrap_or_else(|| "-".into())
                    ),
                    Style::new().fg(Color::DarkGray),
                )),
            ),
            (None, Some(reason)) => lines.insert(
                2,
                Line::from(Span::styled(
                    format!("HOST (no pod): {reason}"),
                    Style::new().fg(Color::Yellow),
                )),
            ),
            (None, None) => {}
        }
        lines
    }

    /// The log's own record of this agent, used when no transcript is available.
    fn log_lines(&self) -> Vec<Line<'static>> {
        let Some(agent) = self.selected_agent() else {
            return Vec::new();
        };
        let mut lines = Vec::new();
        for row in self
            .events
            .iter()
            .filter(|row| string_field(row, "hash_id").as_deref() == Some(&agent.hash_id))
        {
            let event = string_field(row, "event").unwrap_or_default();
            let (glyph, glyph_color) = event_glyph(&event);
            lines.push(Line::from(vec![
                Span::styled(format!("{} ", clock(row)), Style::new().fg(Color::DarkGray)),
                Span::styled(format!("{glyph} {event}"), Style::new().fg(glyph_color)),
                Span::raw(
                    string_field(row, "kind")
                        .map(|kind| format!("  {kind}"))
                        .unwrap_or_default(),
                ),
                Span::raw(
                    string_field(row, "outcome")
                        .map(|outcome| format!("  {outcome}"))
                        .unwrap_or_default(),
                ),
            ]));
            for key in ["message", "reason", "summary"] {
                if let Some(text) = string_field(row, key) {
                    lines.push(Line::from(vec![
                        Span::styled(format!("  {key}: "), Style::new().fg(Color::DarkGray)),
                        Span::raw(text),
                    ]));
                }
            }
            lines.push(Line::raw(""));
        }
        lines
    }

    /// Timeline position as (first visible row, total rows), 1-based for display.
    pub fn position(&self) -> (usize, usize) {
        let total = self.visible_events().len();
        let start = self.window_start(total);
        (if total == 0 { 0 } else { start + 1 }, total)
    }

    /// Scrolls the timeline by whole rows; scrolling up leaves follow mode, and
    /// scrolling to the last row rejoins it so new events keep arriving.
    pub fn scroll_timeline(&mut self, delta: isize) {
        let total = self.visible_events().len();
        let start = self.window_start(total) as isize;
        let last_page = total.saturating_sub(self.viewport) as isize;
        let next = (start + delta).clamp(0, last_page.max(0));
        self.scroll = next as usize;
        self.follow = next >= last_page;
    }

    pub fn viewport_rows(&self) -> usize {
        self.viewport
    }

    /// Last visible row number, 1-based, for the position readout.
    pub fn last_row(&self) -> usize {
        let (first, total) = self.position();
        if total == 0 {
            0
        } else {
            (first + self.viewport - 1).min(total)
        }
    }

    pub fn set_viewport(&mut self, rows: usize) {
        self.viewport = rows.max(1);
        self.clamp_scroll();
    }

    /// Events matching the agent selection and the event filter, in order.
    pub fn visible_events(&self) -> Vec<&Value> {
        let selected = self.selected_agent().map(|agent| agent.hash_id.as_str());
        self.events
            .iter()
            .filter(|row| {
                let event = string_field(row, "event").unwrap_or_default();
                if !self.filter.accepts(&event) {
                    return false;
                }
                match selected {
                    None => true,
                    Some(hash) => string_field(row, "hash_id").as_deref() == Some(hash),
                }
            })
            .collect()
    }

    /// The window of visible events the timeline pane should draw.
    pub fn window(&self) -> Vec<&Value> {
        let rows = self.visible_events();
        let start = self.window_start(rows.len());
        rows.into_iter().skip(start).take(self.viewport).collect()
    }

    fn window_start(&self, total: usize) -> usize {
        let last_page = total.saturating_sub(self.viewport);
        if self.follow {
            last_page
        } else {
            self.scroll.min(last_page)
        }
    }

    fn clamp_scroll(&mut self) {
        let total = self.visible_events().len();
        self.scroll = self.scroll.min(total.saturating_sub(self.viewport));
    }

    /// Opens the selected agent's page: on its live screen while its
    /// session runs, and on its log once it has finished — with the screen
    /// its session left behind still there under the preview tab.
    pub fn open_detail(&mut self, control: &dyn Control) {
        let Some(agent) = self.selected_agent() else {
            return;
        };
        let finished = agent.state() == "exited";
        let session = agent.terminal_handle.clone().filter(|_| !finished);
        let alive = session
            .as_deref()
            .is_some_and(|session| control.is_alive(session));
        self.view = View::Detail;
        // A finished agent has only its log: there is no terminal to watch
        // and its worktree is no longer changing.
        self.tab = if alive { Tab::Live } else { Tab::Log };
        // Open on the newest turn: an operator asks what an agent is doing
        // now, not what it was asked an hour ago.
        self.detail_follow = true;
        self.detail_scroll = 0;
        self.diff = None;
        self.preview = session.filter(|_| alive).map(Preview::new);
        self.preview_area = None;
        self.load_transcript();
        // The page opens watching, not typing: every key would otherwise go straight to the
        // agent. Enter steps in.
        self.typing = false;
    }

    fn close_detail(&mut self) {
        self.view = View::Overview;
        self.typing = false;
        self.pressed = None;
        self.dragged = false;
        self.preview = None;
        self.preview_area = None;
        self.diff = None;
    }

    /// `x` closes the Run on screen. The console's window has no Run, and
    /// what ends the console is a key of its own.
    fn ask_to_close(&mut self) {
        self.closing = true;
    }

    /// The keys that work from anywhere. The checklist is a Run's, so the
    /// console's window, which has no Run, does not offer it.
    fn global_items(&self, esc: Option<&'static str>) -> Vec<menu::Item> {
        let mut items = vec![menu::item("ctrl+\\", "console")];
        if !self.console {
            items.push(menu::item("ctrl+l", "checklist"));
        }
        // While typing, esc and q are the agent's.
        if let Some(esc) = esc {
            items.push(menu::item("esc", esc));
            items.push(menu::item("q", "quit"));
        }
        items
    }

    fn close_item(&self) -> menu::Item {
        if self.console {
            menu::item("x", "stop console")
        } else {
            menu::item("x", "close run")
        }
    }

    pub fn on_key_with(&mut self, key: KeyEvent, control: &dyn Control) -> Action {
        if key.kind == KeyEventKind::Release {
            return Action::Continue;
        }
        // Closing a Run kills live agents, so it asks first and takes only a
        // deliberate yes; every other key means no.
        if self.closing {
            self.closing = false;
            if matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y')) {
                return if self.console {
                    Action::StopConsole
                } else {
                    Action::CloseRun(self.run_id.clone())
                };
            }
            self.notice = Some(Notice::new(if self.console {
                "console left running"
            } else {
                "left running"
            }));
            return Action::Continue;
        }
        if self.view == View::Detail {
            return self.on_detail_key(key, control);
        }
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Esc => return Action::Back,
            // Right mirrors Left in the detail view: the list is a column you
            // step into, so the arrow that leaves it should also enter it.
            KeyCode::Enter | KeyCode::Right => self.open_detail(control),
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Action::Quit;
            }
            KeyCode::Char('x') => self.ask_to_close(),
            KeyCode::Tab | KeyCode::BackTab => self.focus = self.focus.other(),
            KeyCode::Down | KeyCode::Char('j') => match self.focus {
                Focus::Agents => {
                    self.selected = (self.selected + 1).min(self.agents.len());
                    self.clamp_scroll();
                }
                Focus::Timeline => self.scroll_timeline(1),
            },
            KeyCode::Up | KeyCode::Char('k') => match self.focus {
                Focus::Agents => {
                    self.selected = self.selected.saturating_sub(1);
                    self.clamp_scroll();
                }
                Focus::Timeline => self.scroll_timeline(-1),
            },
            KeyCode::Char('e') => {
                self.filter = self.filter.next();
                self.clamp_scroll();
            }
            KeyCode::Char('f') => self.follow = !self.follow,
            KeyCode::Char('g') | KeyCode::Home => {
                self.follow = false;
                self.scroll = 0;
            }
            KeyCode::Char('G') | KeyCode::End => self.follow = true,
            KeyCode::PageUp => self.scroll_timeline(-(self.viewport as isize)),
            KeyCode::PageDown => self.scroll_timeline(self.viewport as isize),
            _ => {}
        }
        Action::Continue
    }

    /// One agent's page: tabs over its screen, its diff and its log.
    fn on_detail_key(&mut self, key: KeyEvent, control: &dyn Control) -> Action {
        // The live tab is the agent's terminal: every key is the agent's,
        // except the one that gives the keys back to this view.
        if self.typing {
            if live::is_leave(&key) {
                self.typing = false;
                return Action::Continue;
            }
            match self.live_session() {
                Some(session) => {
                    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
                    let page = self
                        .preview_area
                        .map(|(_, rows)| rows as isize)
                        .unwrap_or(self.viewport as isize)
                        .max(1);
                    let scrolling = self.preview.as_ref().is_some_and(Preview::scrolling);
                    match key.code {
                        // The two keys kept back for reading the history,
                        // the ones a terminal scrolls its own with.
                        KeyCode::PageUp | KeyCode::PageDown if shift => {
                            let up = key.code == KeyCode::PageUp;
                            match self.scrolled_by_agent(control) {
                                // A full-screen agent pages its own history
                                // with the plain keys.
                                Some((session, _)) => self.type_live(
                                    &session,
                                    LiveInput::Key(if up { "PPage" } else { "NPage" }.to_owned()),
                                    control,
                                ),
                                None => self.live_scroll(control, if up { -page } else { page }),
                            }
                        }
                        // Esc while reading back only stops reading back;
                        // it reaches the agent once the screen is live.
                        KeyCode::Esc if scrolling => {
                            if let Some(preview) = self.preview.as_mut() {
                                preview.leave_scroll();
                            }
                        }
                        _ => {
                            if let Some(input) = live::input_for(&key) {
                                self.type_live(&session, input, control);
                            }
                        }
                    }
                    return Action::Continue;
                }
                None => self.typing = false,
            }
        }
        if self.tab == Tab::Live
            && self.preview.as_ref().is_some_and(Preview::selecting_with_keys)
            && self.on_select_key(key, control)
        {
            return Action::Continue;
        }
        let page = self.viewport.max(1) as isize;
        let shift = key.modifiers.contains(KeyModifiers::SHIFT);
        let on_preview = self.tab == Tab::Live;
        match key.code {
            KeyCode::Char('q') => return Action::Quit,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                return Action::Quit;
            }
            // Esc first leaves the history, then the page; backspace and
            // left leave the page outright.
            KeyCode::Esc => match self.preview.as_mut() {
                Some(preview) if preview.has_selection() => preview.clear_selection(),
                Some(preview) if preview.scrolling() => preview.leave_scroll(),
                _ => self.close_detail(),
            },
            KeyCode::Backspace | KeyCode::Left => self.close_detail(),
            KeyCode::Tab | KeyCode::BackTab => {
                self.tab = self.tab.next_in(self.tabs());
                self.detail_follow = true;
                self.detail_scroll = 0;
                if self.tab == Tab::Diff {
                    self.refresh_diff(control);
                }
                self.typing = false;
            }
            // Into the agent's terminal: the live tab is watched until Enter, which also
            // leaves any reading back, since typing is at the live screen.
            KeyCode::Enter if on_preview => {
                if let Some(preview) = self.preview.as_mut() {
                    preview.leave_scroll();
                }
                self.start_typing();
            }
            KeyCode::Char('x') => self.ask_to_close(),
            KeyCode::Char('v') if on_preview => self.begin_select(control),
            KeyCode::Up | KeyCode::Down if on_preview && shift => {
                self.preview_scroll(control, if key.code == KeyCode::Up { -1 } else { 1 });
            }
            KeyCode::PageUp if on_preview => self.preview_scroll(control, -page),
            KeyCode::PageDown if on_preview => self.preview_scroll(control, page),
            KeyCode::Up | KeyCode::Char('k') if on_preview => self.preview_scroll(control, -1),
            KeyCode::Down | KeyCode::Char('j') if on_preview => self.preview_scroll(control, 1),
            KeyCode::Down | KeyCode::Char('j') => self.scroll_detail(1),
            KeyCode::Up | KeyCode::Char('k') => self.scroll_detail(-1),
            KeyCode::PageDown => self.scroll_detail(page),
            KeyCode::PageUp => self.scroll_detail(-page),
            KeyCode::Char('g') | KeyCode::Home => {
                self.detail_follow = false;
                self.detail_scroll = 0;
            }
            KeyCode::Char('G') | KeyCode::End => self.detail_follow = true,
            _ => {}
        }
        Action::Continue
    }

    /// Reading back: the first scroll takes the history, later ones move
    /// through it.
    fn preview_scroll(&mut self, control: &dyn Control, delta: isize) {
        let viewport = self
            .preview_area
            .map(|(_, rows)| rows as usize)
            .unwrap_or(self.viewport);
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        if !preview.scrolling() {
            preview.enter_scroll(control, viewport);
        }
        preview.scroll(delta);
    }

    /// `v` on the live tab: reading back, with a cursor the keys move to
    /// pick out text.
    fn begin_select(&mut self, control: &dyn Control) {
        self.preview_scroll(control, 0);
        if let Some(preview) = self.preview.as_mut() {
            preview.begin_select();
        }
    }

    /// The keys while selecting with them; `false` leaves a key to the page.
    fn on_select_key(&mut self, key: KeyEvent, control: &dyn Control) -> bool {
        let page = self
            .preview_area
            .map(|(_, rows)| rows as isize)
            .unwrap_or(self.viewport as isize)
            .max(1);
        let Some(preview) = self.preview.as_mut() else {
            return false;
        };
        match key.code {
            KeyCode::Up | KeyCode::Char('k') => preview.move_cursor(-1, 0),
            KeyCode::Down | KeyCode::Char('j') => preview.move_cursor(1, 0),
            KeyCode::Left | KeyCode::Char('h') => preview.move_cursor(0, -1),
            KeyCode::Right | KeyCode::Char('l') => preview.move_cursor(0, 1),
            KeyCode::PageUp => preview.move_cursor(-page, 0),
            KeyCode::PageDown => preview.move_cursor(page, 0),
            KeyCode::Char('g') => preview.move_cursor(isize::MIN / 2, 0),
            KeyCode::Char('G') => preview.move_cursor(isize::MAX / 2, 0),
            KeyCode::Home | KeyCode::Char('0') => preview.cursor_to_edge(false),
            KeyCode::End | KeyCode::Char('$') => preview.cursor_to_edge(true),
            KeyCode::Char('v') | KeyCode::Char(' ') => preview.toggle_mark(),
            KeyCode::Char('y') | KeyCode::Enter => {
                self.copy_selection(control);
                if let Some(preview) = self.preview.as_mut() {
                    preview.clear_selection();
                }
            }
            // Esc puts the selection down and stays reading back.
            KeyCode::Esc => preview.clear_selection(),
            _ => return false,
        }
        true
    }

    /// Puts what is selected on the operator's clipboard, and says so.
    fn copy_selection(&mut self, control: &dyn Control) {
        let Some(text) = self.preview.as_ref().and_then(Preview::selected_text) else {
            return;
        };
        if text.trim().is_empty() {
            self.notice = Some(Notice::new("nothing to copy there"));
            return;
        }
        let lines = text.lines().count();
        self.notice = Some(match control.copy(&text) {
            Ok(()) => Notice::new(format!(
                "copied {lines} line{}",
                if lines == 1 { "" } else { "s" }
            )),
            Err(error) => Notice::alarm(format!("copying failed: {error}")),
        });
    }

    /// Where a terminal cell falls in the live box, when it does.
    fn in_preview(&self, column: u16, row: u16) -> Option<(u16, u16)> {
        if self.view != View::Detail || self.tab != Tab::Live || self.preview.is_none() {
            return None;
        }
        let ((x, y), (cols, rows)) = (self.preview_origin?, self.preview_area?);
        (column >= x && column < x + cols && row >= y && row < y + rows)
            .then(|| (column - x, row - y))
    }

    /// The left button went down: on the live box it may start a selection,
    /// and it puts down the one before.
    pub fn on_press(&mut self, column: u16, row: u16) {
        self.pressed = self.in_preview(column, row);
        self.dragged = false;
        if self.pressed.is_some() {
            if let Some(preview) = self.preview.as_mut() {
                preview.clear_selection();
            }
        }
    }

    /// The button moved while down: the first move stops the screen and
    /// starts selecting where the button went down. Past the box's top or
    /// bottom edge, the history moves under the pointer.
    pub fn on_drag(&mut self, column: u16, row: u16, control: &dyn Control) {
        let (Some((from_col, from_row)), Some((x, y)), Some((cols, rows))) =
            (self.pressed, self.preview_origin, self.preview_area)
        else {
            return;
        };
        if cols == 0 || rows == 0 {
            return;
        }
        if !self.dragged {
            self.dragged = true;
            self.preview_scroll(control, 0);
            if let Some(preview) = self.preview.as_mut() {
                if let Some(at) = preview.point_at(from_row, from_col) {
                    preview.select_from(at);
                }
            }
        }
        let Some(preview) = self.preview.as_mut() else {
            return;
        };
        let row = if row < y {
            preview.scroll(-1);
            0
        } else if row >= y + rows {
            preview.scroll(1);
            rows - 1
        } else {
            row - y
        };
        let col = column.saturating_sub(x).min(cols - 1);
        if let Some(at) = preview.point_at(row, col) {
            preview.select_to(at);
        }
    }

    /// The button came up: a drag has selected something, which is copied
    /// straight away and stays marked until the next click or esc.
    pub fn on_release(&mut self, control: &dyn Control) {
        let dragged = std::mem::take(&mut self.dragged);
        if self.pressed.take().is_some() && dragged {
            self.copy_selection(control);
        }
    }

    /// Text the terminal delivered in one piece: the agent's, while typing.
    pub fn on_paste(&mut self, text: &str, control: &dyn Control) {
        if !self.typing {
            return;
        }
        if let Some(session) = self.live_session() {
            self.type_live(&session, LiveInput::Paste(text.to_owned()), control);
        }
    }

    /// The wheel drives whichever screen is showing.
    pub fn on_scroll(&mut self, up: bool, control: &dyn Control) {
        let delta = if up { -3 } else { 3 };
        match (self.view, self.tab) {
            (View::Overview, _) => self.scroll_timeline(delta),
            (View::Detail, Tab::Live) => self.live_wheel(up, control),
            (View::Detail, _) => self.scroll_detail(delta),
        }
    }

    /// The session and how its program draws, when that program keeps its
    /// own history: on the alternate screen tmux keeps none, so reading
    /// back is the program's job and the scrolling is handed to it.
    fn scrolled_by_agent(&self, control: &dyn Control) -> Option<(String, live::ScreenMode)> {
        if self.preview.as_ref().is_some_and(Preview::scrolling) {
            return None;
        }
        let session = self.live_session()?;
        let mode = control.screen_mode(&session)?;
        mode.alternate.then_some((session, mode))
    }

    /// The wheel on the live screen: to the agent when it scrolls itself,
    /// to tmux's history otherwise.
    fn live_wheel(&mut self, up: bool, control: &dyn Control) {
        match self.scrolled_by_agent(control) {
            Some((session, mode)) => {
                let input = if mode.mouse {
                    let (cols, rows) = self.preview_area.unwrap_or((80, 24));
                    live::wheel(up, cols / 2 + 1, rows / 2 + 1)
                } else {
                    // What a terminal sends a full-screen program that did
                    // not ask for the mouse.
                    LiveInput::Key(if up { "Up" } else { "Down" }.to_owned())
                };
                self.type_live(&session, input, control);
            }
            None => self.live_scroll(control, if up { -3 } else { 3 }),
        }
    }

    /// Reads back through the live screen's history, typing or not. Going
    /// down past the newest line is going back to the live screen, so the
    /// wheel alone gets there and back.
    fn live_scroll(&mut self, control: &dyn Control, delta: isize) {
        let scrolling = self.preview.as_ref().is_some_and(Preview::scrolling);
        if delta > 0 && !scrolling {
            return;
        }
        self.preview_scroll(control, delta);
        if delta > 0 {
            if let Some(preview) = self.preview.as_mut() {
                if preview.at_bottom() {
                    preview.leave_scroll();
                }
            }
        }
    }

    /// The key hints for wherever the view stands.
    pub fn menu(&self) -> Vec<Vec<menu::Item>> {
        use menu::item;
        if self.typing {
            return vec![
                vec![
                    item("ctrl+]", "leave live"),
                    item("keys", "go to the agent"),
                    item("wheel/shift+PgUp", "scroll back"),
                ],
                self.global_items(None),
            ];
        }
        if self.closing && self.console {
            return vec![vec![
                item("y", "stop the console"),
                item("any other key", "cancel"),
            ]];
        }
        if self.closing {
            let live = self
                .agents
                .iter()
                .filter(|agent| agent.state() == "active")
                .count();
            return vec![vec![
                item(
                    "y",
                    format!(
                        "close {} and stop {live} live agent{}",
                        self.run_id,
                        if live == 1 { "" } else { "s" }
                    ),
                ),
                item("any other key", "cancel"),
            ]];
        }
        match self.view {
            View::Overview => vec![
                vec![
                    item(
                        "tab",
                        match self.focus {
                            Focus::Agents => "pane:agents",
                            Focus::Timeline => "pane:timeline",
                        },
                    ),
                    item(
                        "↑↓",
                        match self.focus {
                            Focus::Agents => "agent",
                            Focus::Timeline => "scroll",
                        },
                    ),
                    item("e", format!("events:{}", self.filter().label())),
                    item(
                        "f",
                        format!("follow:{}", if self.following() { "on" } else { "off" }),
                    ),
                ],
                vec![item("enter", "open live"), self.close_item()],
                self.global_items(Some("runs")),
            ],
            View::Detail => {
                let scrolling = self.preview.as_ref().is_some_and(Preview::scrolling);
                let mut first = Vec::new();
                if self.tabs().len() > 1 {
                    first.push(item("tab", format!("tab:{}", self.tab.label())));
                }
                let keys = self.preview.as_ref().is_some_and(Preview::selecting_with_keys);
                match self.tab {
                    Tab::Live if keys => {
                        first.push(item("↑↓←→", "move"));
                        first.push(item("v", "mark"));
                        first.push(item("y", "copy"));
                        first.push(item("esc", "cancel"));
                    }
                    Tab::Live if scrolling => {
                        first.push(item("↑↓/PgUp/PgDn", "history"));
                        first.push(item("v/drag", "select"));
                        first.push(item("esc", "exit scroll"));
                    }
                    Tab::Live => {
                        first.push(item("enter", "type"));
                        first.push(item("shift+↑↓", "scroll back"));
                        first.push(item("v/drag", "select"));
                    }
                    _ => {
                        first.push(item("↑↓", "scroll"));
                        first.push(item("g/G", "top/end"));
                    }
                }
                vec![
                    first,
                    vec![self.close_item()],
                    self.global_items(Some("back")),
                ]
            }
        }
    }
}

/// Whether the roster shows its rule: only when it has both live agents and
/// finished ones to separate. The height and the list have to agree about
/// this or the list is taller than the space it is given.
fn roster_rule(state: &TuiState) -> bool {
    let live = state
        .agents()
        .iter()
        .take_while(|agent| agent.state() == "active")
        .count();
    live > 0 && live < state.agents().len()
}

pub fn draw(frame: &mut Frame, state: &mut TuiState) {
    let areas = Layout::vertical([
        Constraint::Length(1),
        // The roster holds a row per agent, the "all agents" row, and the
        // rule between the live ones and the finished ones.
        Constraint::Length(
            (state.agents.len() as u16 + 1 + roster_rule(state) as u16).clamp(2, 9) + 2,
        ),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .split(frame.area());

    let active = state
        .agents()
        .iter()
        .filter(|agent| agent.state() == "active")
        .count();
    let total = state.spend_total();
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {} ", state.run_id),
                Style::new().fg(Color::Black).bg(Color::Cyan),
            ),
            Span::raw(format!(
                "  {} agents ({active} active)  {} events",
                state.agents.len(),
                state.events.len(),
            )),
            Span::styled(
                match total.cost() {
                    Some(cost) => format!(
                        "  {} tokens  {}",
                        pricing::human_tokens(total.tokens().total()),
                        pricing::human_money(cost)
                    ),
                    None => String::new(),
                },
                Style::new().fg(Color::Yellow),
            ),
            Span::raw(format!("  times {}", model::local_offset())),
        ])),
        areas[0],
    );

    let column = state.hash_column();
    let mut rows: Vec<ListItem> = vec![ListItem::new(Line::from(Span::styled(
        "all agents",
        Style::new().fg(Color::Gray),
    )))];
    // The coordinator is the one who asks, so its row is where a reader is
    // sent to read the question in full and answer it.
    let waiting = state.open_question().is_some();
    let now = Instant::now();
    let now_ms = event_log::now_ms() as i64;
    rows.extend(state.agents().iter().map(|agent| {
        let color = role_color(agent.role.as_deref());
        let asking = waiting && agent.is_meta() && agent.state() == "active";
        let (mark, mark_color) = if asking {
            (WAITING_MARK, Color::Yellow)
        } else {
            state.status_glyph(agent, now, now_ms)
        };
        let name = Style::new().fg(color).add_modifier(Modifier::BOLD);
        let mut spans = vec![
            Span::styled(format!("{mark} "), Style::new().fg(mark_color)),
            Span::styled(format!("{:<width$}", agent.hash_id, width = column), name),
            Span::styled(
                format!("{:<10}", agent.role.clone().unwrap_or_else(|| "-".into())),
                Style::new().fg(color),
            ),
            Span::styled(
                // How long it has worked, which the clock beside it does not
                // say: an agent that entered at 12:23 and one still going
                // since 09:41 read the same otherwise.
                format!(
                    "{:>7}  ",
                    agent
                        .seconds()
                        .map(model::human_span)
                        .unwrap_or_default()
                ),
                Style::new().fg(if agent.state() == "active" {
                    Color::White
                } else {
                    Color::DarkGray
                }),
            ),
            Span::styled(
                format!(
                    "{:<18}",
                    state
                        .spend_of(&agent.hash_id)
                        .map(model_and_effort)
                        .unwrap_or_default()
                ),
                Style::new().fg(Color::White),
            ),
            Span::styled(
                match state.spend_of(&agent.hash_id) {
                    Some(session) => format!(
                        "{:>6} {:>8}  ",
                        pricing::human_tokens(session.usage.tokens().total()),
                        session
                            .usage
                            .cost()
                            .map(pricing::human_money)
                            .unwrap_or_default()
                    ),
                    None => format!("{:>6} {:>8}  ", "-", ""),
                },
                Style::new().fg(Color::White),
            ),
            Span::styled(
                format!("{:<20}", agent_status(agent)),
                Style::new().fg(status_color(agent)),
            ),
        ];
        // What its worktree has changed so far, the way a diff stat reads.
        if let Some((added, removed)) = state.diff_stat_of(&agent.hash_id) {
            spans.push(Span::styled(
                format!("+{added}"),
                Style::new().fg(Color::Green),
            ));
            spans.push(Span::styled(",", Style::new().fg(Color::DarkGray)));
            spans.push(Span::styled(
                format!("-{removed}"),
                Style::new().fg(Color::Red),
            ));
        }
        // Where its commands run: a pod by name, or the host when it asked for a pod and did
        // not get one — said on the row rather than left to be inferred.
        match (agent.env_id.as_deref(), agent.env_skipped.is_some()) {
            (Some(_), _) => spans.push(Span::styled(
                format!("  {}", agent.exec()),
                Style::new().fg(Color::Blue),
            )),
            (None, true) => spans.push(Span::styled(
                format!("  {}", agent.exec()),
                Style::new().fg(Color::Yellow),
            )),
            (None, false) => {}
        }
        ListItem::new(Line::from(spans))
    }));
    // The list holds one non-selectable rule between the groups, so the
    // highlighted row is the selection's position after that shift.
    let live = state
        .agents()
        .iter()
        .take_while(|agent| agent.state() == "active")
        .count();
    let rule_after = roster_rule(state).then_some(live);
    if let Some(at) = rule_after {
        rows.insert(
            at + 1,
            ListItem::new(Line::from(Span::styled(
                "─".repeat(6) + " finished " + &"─".repeat(60),
                Style::new().fg(Color::DarkGray),
            ))),
        );
    }
    let highlighted = match rule_after {
        Some(at) if state.selected > at => state.selected + 1,
        _ => state.selected,
    };
    let mut list_state = ListState::default().with_selected(Some(highlighted));
    frame.render_stateful_widget(
        List::new(rows)
            .block(pane(
                " agents ",
                state.view() == View::Overview && state.focus() == Focus::Agents,
            ))
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED)),
        areas[1],
        &mut list_state,
    );

    if state.view() == View::Detail {
        // One agent's page takes the timeline's place; the roster above
        // stays visible and keeps updating, it just cannot take the arrows.
        tabbed::draw_detail(frame, areas[2], state);
    } else {
        draw_timeline(frame, areas[2], state);
    }

    let footer = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(areas[3]);
    frame.render_widget(
        Paragraph::new(menu::line(&state.menu(), footer[0].width)),
        footer[0],
    );
    menu::draw_notice(frame, footer[1], state.notice.as_ref());
}

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
    let title = match state.selected_agent() {
        Some(agent) => format!(" timeline · {} ", agent.hash_id),
        None => " timeline ".to_owned(),
    };
    frame.render_widget(
        Paragraph::new(timeline).block(
            pane(&title, state.focus() == Focus::Timeline).title_bottom(
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

/// Ctrl+\, or F2 for a terminal that swallows it: the console from
/// anywhere. No agent binds Ctrl+\ — Ctrl+O, which this once was, is Claude
/// Code's own — so it can be taken even while typing on the live tab. A
/// terminal without the enhanced keyboard reports it as Ctrl+4.
pub fn is_console_toggle(key: &KeyEvent) -> bool {
    if key.kind == KeyEventKind::Release {
        return false;
    }
    matches!(key.code, KeyCode::Char('\\') | KeyCode::Char('4') if key.modifiers.contains(KeyModifiers::CONTROL))
        || matches!(key.code, KeyCode::F(2))
}

/// Runs the view until the operator quits. Restores the terminal on every exit path,
/// including an error inside the loop.
pub fn run(env: &dyn Environment) -> Result<()> {
    let store = Store::open(env)?;
    let log = EventLog::open(env);
    let checklist = ChecklistStore::open(env);
    let control = RealControl { env };
    let projects_root = transcript::claude_projects_dir(env);
    let mut terminal = enter()?;
    let outcome = event_loop(
        &mut terminal,
        &store,
        &log,
        projects_root,
        &control,
        Some(&checklist),
    );
    leave(&mut terminal)?;
    outcome
}

type Tui = Terminal<CrosstermBackend<Stdout>>;

/// Reports mouse buttons, the wheel among them, in SGR form (DECSET 1002 and
/// 1006). The wheel has to arrive as a wheel: turned into arrow keys, as
/// "alternate scroll" does, it would be typed at the agent on the live tab
/// instead of scrolling it. Motion is reported only while a button is held,
/// which is what a drag across the live screen selects with; moving the
/// mouse otherwise costs nothing. Most terminals still select text their own
/// way with Shift held down.
const MOUSE_REPORTING_ON: &str = "\x1b[?1002h\x1b[?1006h";
const MOUSE_REPORTING_OFF: &str = "\x1b[?1006l\x1b[?1002l";

/// Whether the terminal was asked to report keys the way the prompt box
/// wants them. Remembered rather than asked twice: by the time the view is
/// leaving, raw mode is already off and there is nothing left to answer it.
static ENHANCED: AtomicBool = AtomicBool::new(false);

fn enter() -> Result<Tui> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    enter_screen(&mut stdout)?;
    Terminal::new(CrosstermBackend::new(stdout)).context("failed to start the terminal backend")
}

fn enter_screen(stdout: &mut impl io::Write) -> Result<()> {
    // Bracketed paste so a pasted diff arrives as one piece of text rather
    // than as a burst of keystrokes with its newlines read as sends.
    execute!(
        stdout,
        EnterAlternateScreen,
        EnableBracketedPaste,
        Print(MOUSE_REPORTING_ON)
    )
    .context("failed to enter the alternate screen")?;
    // And, where the terminal can, the flag that tells shift+enter from
    // enter. Terminals that cannot are why the backslash fallback exists.
    if supports_keyboard_enhancement().unwrap_or(false)
        && execute!(
            stdout,
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )
        .is_ok()
    {
        ENHANCED.store(true, Ordering::Relaxed);
    }
    Ok(())
}

fn leave(terminal: &mut Tui) -> Result<()> {
    disable_raw_mode().context("failed to restore the terminal mode")?;
    if ENHANCED.swap(false, Ordering::Relaxed) {
        let _ = execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags);
    }
    execute!(
        terminal.backend_mut(),
        Print(MOUSE_REPORTING_OFF),
        DisableBracketedPaste,
        LeaveAlternateScreen
    )
    .context("failed to leave the alternate screen")?;
    terminal.show_cursor().context("failed to show the cursor")
}

fn open_run(store: &Store, log: &EventLog, run_id: &str, projects_root: Option<PathBuf>) -> Result<TuiState> {
    Ok(TuiState::with_projects(
        RunSnapshot::read(store, log, run_id)?,
        projects_root,
    ))
}

/// The console's window. It is drawn like a Run's so the console is watched and typed at
/// like any agent, but its one agent comes from the console's record: the console belongs to
/// no Run (ADR-0005) and is in no Run's log.
pub fn console_state(console: &ConsoleHandle, projects_root: Option<PathBuf>) -> TuiState {
    let name = console
        .repo
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_owned());
    let agent = AgentRow {
        hash_id: format!("console-{name}"),
        role: Some(crate::role::CoreRole::Console.name().to_owned()),
        agent: Some(console.backend.clone()),
        worktree: console.dir.as_ref().map(|dir| dir.display().to_string()),
        terminal_handle: Some(console.session.clone()),
        entered: Some(console.started_at.clone()),
        ..AgentRow::default()
    };
    let entered = serde_json::json!({
        "time": console.started_at,
        "event": events::AGENT_ENTER,
        "hash_id": agent.hash_id,
        "role": agent.role,
        "backend": console.backend,
        "repo": console.repo.display().to_string(),
    });
    let mut state = TuiState::empty("console".to_owned(), projects_root);
    state.repo = Some(console.repo.clone());
    state.backend = console.backend.clone();
    state.console = true;
    state.refresh_rows(vec![agent], vec![entered]);
    state
}

/// A console the loop is about to open, once the frame saying so is on screen: opening one
/// that is not running launches it, which takes seconds and blocks the keyboard meanwhile.
struct PendingConsole {
    repo: PathBuf,
    /// It replaces the console already on screen instead of opening over whatever was there.
    in_place: bool,
}

/// The repository `ctrl+\\` opens a console for: the one the view was started in (ADR-0005),
/// found from the git top level so a subdirectory reaches it too, whichever Run is on screen.
fn console_target(start: &Path) -> PathBuf {
    let repo = crate::worktree::run_git(start, &["rev-parse", "--show-toplevel"])
        .ok()
        .filter(|output| output.status.success())
        .map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
        .filter(|repo| !repo.as_os_str().is_empty())
        .unwrap_or_else(|| start.to_path_buf());
    repo
}

fn event_loop(
    terminal: &mut Tui,
    store: &Store,
    log: &EventLog,
    projects_root: Option<PathBuf>,
    control: &dyn Control,
    checklist_store: Option<&ChecklistStore>,
) -> Result<()> {
    let mut picker = Picker::new(model::run_summaries(store, log));
    let console_repo = std::env::current_dir().ok().map(|dir| console_target(&dir));
    let mut usage = SpendIndex::default();
    // Reading every Run's transcripts once fills the picker's costs; the index then re-reads
    // only what changes.
    let mut spend: HashMap<String, Usage> = HashMap::new();
    let mut open: Option<TuiState> = None;
    // The screen the console was opened over, to come back to.
    let mut parked: Option<TuiState> = None;
    let mut checklist_panel = ChecklistPanel::new();
    let mut pending: Option<PendingConsole> = None;
    let mut picker_notice: Option<Notice> = None;
    // Zero so the first pass fills the picker's costs straight away.
    let mut polled = Instant::now() - POLL_INTERVAL;
    let mut previewed = Instant::now();
    loop {
        terminal.draw(|frame| {
            match open.as_mut() {
                Some(state) => draw(frame, state),
                None => {
                    draw_runs(frame, &picker, log.dir(), &spend);
                    let area = frame.area();
                    let notice_row = Rect {
                        y: area.bottom().saturating_sub(1),
                        height: 1.min(area.height),
                        ..area
                    };
                    if picker.notice().is_none() {
                        menu::draw_notice(frame, notice_row, picker_notice.as_ref());
                    }
                }
            }
            if checklist_panel.is_open() {
                checklist_panel::draw(frame, &checklist_panel);
            }
        })?;
        if let Some(state) = open.as_mut() {
            state.fit_preview(control);
        }
        // The frame saying the console is starting is on screen; now start it.
        if let Some(console) = pending.take() {
            let notice = match control.console(&console.repo) {
                Ok(handle) => {
                    let mut state = console_state(&handle, projects_root.clone());
                    state.refresh_spend(&mut usage);
                    state.refresh_sessions(control);
                    state.selected = state.agents().len().min(1);
                    state.open_detail(control);
                    // Restarted from inside the console, it replaces the console on screen;
                    // what the console was opened over stays parked beneath it.
                    if console.in_place {
                        open = Some(state);
                    } else {
                        parked = open.replace(state);
                    }
                    None
                }
                Err(error) => Some(Notice::alarm(format!("console could not start: {error}"))),
            };
            if let Some(notice) = notice {
                match open.as_mut() {
                    Some(state) => state.set_notice(notice),
                    None => picker_notice = Some(notice),
                }
            }
            continue;
        }
        let in_console = open.as_ref().is_some_and(|state| state.console);
        let timeout = if open.as_ref().is_some_and(TuiState::preview_visible) {
            PREVIEW_INTERVAL
        } else {
            INPUT_TIMEOUT
        };
        if event::poll(timeout)? {
            match event::read()? {
                // The console answers from anywhere, so its key is tested before anything
                // else claims one.
                Event::Key(key) if is_console_toggle(&key) => {
                    if in_console {
                        // Already there: the key is a toggle, so it goes back to whatever
                        // was open before.
                        open = parked.take();
                    } else if let Some(repo) = console_repo.clone() {
                        let notice = Notice::new(if control.console_live(&repo) {
                            "opening the console…"
                        } else {
                            "starting the console…"
                        });
                        match open.as_mut() {
                            Some(state) => state.set_notice(notice),
                            None => picker_notice = Some(notice),
                        }
                        pending = Some(PendingConsole {
                            repo,
                            in_place: false,
                        });
                    }
                }
                // Only meaningful next to a Run, so unlike the console this one is silent
                // everywhere else, including the picker.
                Event::Key(key)
                    if checklist_panel::is_toggle(&key) && open.is_some() && !in_console =>
                {
                    if checklist_panel.is_open() {
                        checklist_panel.close();
                    } else if let Some(state) = open.as_ref() {
                        checklist_panel.open_for(state.run_id.clone());
                        checklist_panel.refresh(checklist_store);
                    }
                }
                Event::Key(key) if checklist_panel.is_open() => {
                    if matches!(key.code, KeyCode::Esc) {
                        checklist_panel.close();
                    }
                }
                Event::Key(key) => match open.as_mut() {
                    Some(state) => match state.on_key_with(key, control) {
                        Action::Quit => return Ok(()),
                        Action::Back => {
                            // Leaving the console goes back to what it was opened over, not
                            // to the picker.
                            open = if in_console { parked.take() } else { None };
                            if open.is_none() {
                                picker.refresh(model::run_summaries(store, log));
                            }
                        }
                        Action::Continue => {}
                        Action::StopConsole => {
                            if let Some(repo) = state.repo.clone() {
                                let outcome = match control.stop_console(&repo) {
                                    Ok(summary) => Notice::new(format!("console {summary}")),
                                    Err(error) => Notice::alarm(format!("console: {error}")),
                                };
                                open = parked.take();
                                match open.as_mut() {
                                    Some(state) => state.set_notice(outcome),
                                    None => picker_notice = Some(outcome),
                                }
                            }
                        }
                        Action::CloseRun(run_id) => {
                            let outcome = match control.close_run(&run_id) {
                                Ok(summary) => Notice::new(format!("{run_id} {summary}")),
                                Err(error) => Notice::alarm(format!(
                                    "{run_id} could not be closed: {error}"
                                )),
                            };
                            state.set_notice(outcome);
                        }
                    },
                    None => match picker.on_key(key, control) {
                        PickerAction::Quit => return Ok(()),
                        PickerAction::Open(run_id) => {
                            let mut state =
                                open_run(store, log, &run_id, projects_root.clone())?;
                            state.refresh_spend(&mut usage);
                            state.refresh_sessions(control);
                            open = Some(state);
                        }
                        PickerAction::Continue => {}
                    },
                },
                Event::Paste(text) => {
                    if let Some(state) = open.as_mut() {
                        state.on_paste(&text, control);
                    }
                }
                Event::Mouse(mouse) if !checklist_panel.is_open() && open.is_some() => {
                    let state = open.as_mut().expect("checked above");
                    let (column, row) = (mouse.column, mouse.row);
                    match mouse.kind {
                        MouseEventKind::ScrollUp => state.on_scroll(true, control),
                        MouseEventKind::ScrollDown => state.on_scroll(false, control),
                        MouseEventKind::Down(MouseButton::Left) => state.on_press(column, row),
                        MouseEventKind::Drag(MouseButton::Left) => {
                            state.on_drag(column, row, control)
                        }
                        MouseEventKind::Up(MouseButton::Left) => state.on_release(control),
                        _ => {}
                    }
                }
                Event::Mouse(mouse) => {
                    let up = match mouse.kind {
                        MouseEventKind::ScrollUp => Some(true),
                        MouseEventKind::ScrollDown => Some(false),
                        _ => None,
                    };
                    if let Some(up) = up {
                        if let Some(state) = open.as_mut() {
                            state.on_scroll(up, control);
                        } else {
                            picker.list.move_cursor(if up { -1 } else { 1 });
                        }
                    }
                }
                _ => {}
            }
        }
        // The console is the one agent that is meant to always be there: typing at one whose
        // session has gone starts it again in its place rather than leaving a screen nothing
        // can reach.
        if let Some(state) = open.as_mut() {
            if std::mem::take(&mut state.session_lost) && state.console && pending.is_none() {
                if let Some(repo) = state.repo.clone() {
                    state.set_notice(Notice::new("the console's session is gone; restarting it…"));
                    pending = Some(PendingConsole {
                        repo,
                        in_place: true,
                    });
                }
            }
        }
        if previewed.elapsed() >= PREVIEW_INTERVAL {
            previewed = Instant::now();
            if let Some(state) = open.as_mut() {
                state.refresh_preview(control);
            }
        }
        if polled.elapsed() >= POLL_INTERVAL {
            polled = Instant::now();
            if checklist_panel.is_open() {
                checklist_panel.refresh(checklist_store);
            }
            match open.as_mut() {
                Some(state) => {
                    // The console's window is built from its record, not read from a log: it
                    // has no Run to read.
                    if !state.console {
                        if let Ok(snapshot) = RunSnapshot::read(store, log, &state.run_id.clone()) {
                            state.refresh(snapshot);
                        }
                    }
                    state.refresh_spend(&mut usage);
                    state.refresh_sessions(control);
                }
                None => {
                    picker.refresh(model::run_summaries(store, log));
                    for run in picker.list.runs() {
                        if let Ok(snapshot) = RunSnapshot::read(store, log, &run.run_id) {
                            let run_usage =
                                usage.of_run(projects_root.as_deref(), &snapshot.agents());
                            spend.insert(run.run_id.clone(), run_usage);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
