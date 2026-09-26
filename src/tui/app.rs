//! The live view's state and rendering (ticket Scope: the Run picker, the per-Run roster,
//! each agent's live tab, its log and diff, the checklist panel, closing a Run, and the
//! needs-human marker). Every read comes from the Store, the workflow log, and the checklist
//! store — files an observer may always read — and everything that touches a live agent goes
//! through `Control`, so this whole module renders and reacts to keys with no tmux or git call
//! of its own, which is what makes it testable against recorded Runs and a stand-in control.

use crate::checklist::ChecklistStore;
use crate::event_log::{events, EventLog, LogEntry};
use crate::store::Store;
use crate::tui::control::Control;
use anyhow::Result;
use crossterm::event::KeyCode;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Tabs};
use ratatui::Frame;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentTab {
    Live,
    Log,
    Diff,
    Checklist,
}

impl AgentTab {
    fn next(self) -> Self {
        match self {
            AgentTab::Live => AgentTab::Log,
            AgentTab::Log => AgentTab::Diff,
            AgentTab::Diff => AgentTab::Checklist,
            AgentTab::Checklist => AgentTab::Live,
        }
    }

    fn label(self) -> &'static str {
        match self {
            AgentTab::Live => "Live",
            AgentTab::Log => "Log",
            AgentTab::Diff => "Diff",
            AgentTab::Checklist => "Checklist",
        }
    }

    fn index(self) -> usize {
        match self {
            AgentTab::Live => 0,
            AgentTab::Log => 1,
            AgentTab::Diff => 2,
            AgentTab::Checklist => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Picker,
    Roster,
    Agent(AgentTab),
    /// The current Run's repository console (ADR-0005), viewed and typed into the same way an
    /// agent's live tab is (ticket Scope: "opening, leaving and stopping it from the live
    /// view").
    Console,
}

#[derive(Debug, Clone)]
pub struct RunRow {
    pub id: String,
    pub open: bool,
    pub needs_human: bool,
    /// `pod:<profile>`, `host`, or empty for a Run older than the exec events.
    pub exec: String,
}

#[derive(Debug, Clone)]
pub struct DispatchRow {
    pub id: String,
    pub role: String,
    pub worktree: String,
    pub session: String,
    pub alive: bool,
    pub settled: bool,
    /// `pod:<env_id>`, `HOST (no pod)` for a role that asked for one and did not get it, or
    /// empty for a role that never asked.
    pub exec: String,
}

/// Where a Run's roles run their commands, read from the workflow log's exec events.
fn run_exec(entries: &[LogEntry]) -> String {
    entries
        .iter()
        .rev()
        .find_map(|entry| match entry.event.as_str() {
            events::EXEC_PROFILE_SELECTED => {
                let profile = entry.details.as_ref()?.get("profile")?.as_str()?.to_string();
                Some(format!("pod:{profile}"))
            }
            events::EXEC_PROFILE_NONE => Some("host".to_string()),
            _ => None,
        })
        .unwrap_or_default()
}

fn dispatch_exec(dispatch: &crate::store::DispatchRecord) -> String {
    match (&dispatch.env_id, &dispatch.env_skipped) {
        (Some(env_id), _) => format!("pod:{env_id}"),
        (None, Some(_)) => "HOST (no pod)".to_string(),
        (None, None) => String::new(),
    }
}

/// A run has an open "needs a human" marker once a `needs-human` log entry has no later
/// `answered` entry (`.dev_docs/CONTEXT.md`, workflow log is the only place an observer reads a
/// Run from).
fn needs_human_open(entries: &[LogEntry]) -> bool {
    let mut open = false;
    for entry in entries {
        if entry.event == events::NEEDS_HUMAN {
            open = true;
        } else if entry.event == events::HUMAN_ANSWER {
            open = false;
        }
    }
    open
}

fn clamp_cursor(current: usize, delta: i32, len: usize) -> usize {
    if len == 0 {
        return 0;
    }
    let next = current as i32 + delta;
    next.clamp(0, len as i32 - 1) as usize
}

pub struct App<'a> {
    store: &'a Store,
    log: &'a EventLog,
    checklist: &'a ChecklistStore,
    control: &'a dyn Control,
    pub runs: Vec<RunRow>,
    pub dispatches: Vec<DispatchRow>,
    pub focus: Focus,
    pub run_cursor: usize,
    pub dispatch_cursor: usize,
    pub live_input: String,
    pub status_line: String,
    pub should_quit: bool,
    pub last_size: Option<(u16, u16)>,
    pub console_repo: Option<PathBuf>,
    pub console_session: Option<String>,
}

impl<'a> App<'a> {
    pub fn new(
        store: &'a Store,
        log: &'a EventLog,
        checklist: &'a ChecklistStore,
        control: &'a dyn Control,
    ) -> Result<Self> {
        let mut app = Self {
            store,
            log,
            checklist,
            control,
            runs: Vec::new(),
            dispatches: Vec::new(),
            focus: Focus::Picker,
            run_cursor: 0,
            dispatch_cursor: 0,
            live_input: String::new(),
            status_line: String::new(),
            should_quit: false,
            last_size: None,
            console_repo: None,
            console_session: None,
        };
        app.reload_runs()?;
        Ok(app)
    }

    pub fn reload_runs(&mut self) -> Result<()> {
        let mut runs = Vec::new();
        for id in self.store.list_run_ids()? {
            let record = self.store.load_run(&id)?;
            let entries = self.log.read_run(&id)?;
            runs.push(RunRow {
                id,
                open: record.is_open(),
                needs_human: needs_human_open(&entries),
                exec: run_exec(&entries),
            });
        }
        self.runs = runs;
        if self.run_cursor >= self.runs.len() {
            self.run_cursor = self.runs.len().saturating_sub(1);
        }
        Ok(())
    }

    fn selected_run(&self) -> Option<&RunRow> {
        self.runs.get(self.run_cursor)
    }

    fn load_dispatches(&self, run_id: &str) -> Result<Vec<DispatchRow>> {
        let run_record = self.store.load_run(run_id)?;
        let dir = self.store.root().join(run_id).join("dispatches");
        let mut rows = Vec::new();
        if dir.exists() {
            for entry in std::fs::read_dir(&dir)? {
                let entry = entry?;
                let dispatch_id = entry.file_name().to_string_lossy().to_string();
                let Ok(dispatch) = self.store.load_dispatch(run_id, &dispatch_id) else {
                    continue;
                };
                let hid = crate::event_log::hash_id(&dispatch.role, &run_record.name, &dispatch_id);
                let session = crate::session::tmux::Tmux::session_name(&hid);
                let alive = dispatch.released_at.is_none() && self.control.session_alive(&session);
                rows.push(DispatchRow {
                    id: dispatch_id,
                    role: dispatch.role.clone(),
                    worktree: dispatch.worktree.clone(),
                    session,
                    alive,
                    settled: dispatch.is_settled(),
                    exec: dispatch_exec(&dispatch),
                });
            }
        }
        rows.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(rows)
    }

    pub fn enter_roster(&mut self) -> Result<()> {
        let Some(run) = self.selected_run() else {
            return Ok(());
        };
        let run_id = run.id.clone();
        self.dispatches = self.load_dispatches(&run_id)?;
        self.dispatch_cursor = 0;
        self.focus = Focus::Roster;
        Ok(())
    }

    pub fn enter_agent(&mut self) {
        if let Some(d) = self.dispatches.get(self.dispatch_cursor) {
            self.live_input.clear();
            self.focus = Focus::Agent(AgentTab::Live);
            if let Some((cols, rows)) = self.last_size {
                let _ = self.control.resize(&d.session, cols, rows);
            }
        }
    }

    /// Records the live view's current viewport, so entering the live tab can resize the
    /// session's window to match it (the control seam's `resize`).
    pub fn set_viewport(&mut self, cols: u16, rows: u16) {
        self.last_size = Some((cols, rows));
    }

    pub fn back(&mut self) {
        self.focus = match self.focus {
            Focus::Agent(_) => Focus::Roster,
            Focus::Roster => Focus::Picker,
            Focus::Picker => Focus::Picker,
            Focus::Console => Focus::Picker,
        };
    }

    fn next_tab(&mut self) {
        if let Focus::Agent(tab) = self.focus {
            self.focus = Focus::Agent(tab.next());
        }
    }

    fn move_cursor(&mut self, delta: i32) {
        match self.focus {
            Focus::Picker => self.run_cursor = clamp_cursor(self.run_cursor, delta, self.runs.len()),
            Focus::Roster => {
                self.dispatch_cursor = clamp_cursor(self.dispatch_cursor, delta, self.dispatches.len())
            }
            Focus::Agent(_) | Focus::Console => {}
        }
    }

    /// Closes the selected Run the way `meta finish` would (ticket Scope: "closing a Run"),
    /// through the control seam so a test never runs the real lifecycle.
    pub fn close_selected_run(&mut self) -> Result<()> {
        if let Some(run) = self.selected_run() {
            let run_id = run.id.clone();
            self.control.close_run(&run_id)?;
            self.status_line = format!("closed {run_id}");
            self.reload_runs()?;
            self.focus = Focus::Picker;
        }
        Ok(())
    }

    /// Opens the console of the selected Run's repository (ADR-0005) and switches focus to it
    /// — the more natural of the two repositories a live-view action could pick, since the
    /// operator already picked a Run and the picker has no other notion of "current
    /// repository". Reopening one already open is the no-op `console::open` already makes it.
    pub fn open_console(&mut self) -> Result<()> {
        let Some(run) = self.selected_run() else {
            return Ok(());
        };
        let run_id = run.id.clone();
        let record = self.store.load_run(&run_id)?;
        let repo = PathBuf::from(&record.repo);
        let session = self.control.open_console(&repo, &record.backend)?;
        if let Some((cols, rows)) = self.last_size {
            let _ = self.control.resize(&session, cols, rows);
        }
        self.console_repo = Some(repo);
        self.console_session = Some(session);
        self.live_input.clear();
        self.focus = Focus::Console;
        Ok(())
    }

    /// Leaves the console view without stopping its session (ticket Scope: "leaving ... it").
    pub fn leave_console(&mut self) {
        self.focus = Focus::Picker;
    }

    /// Stops the open console's session (ticket Scope: "stopping it") and leaves its view.
    pub fn stop_console(&mut self) -> Result<()> {
        if let Some(repo) = self.console_repo.clone() {
            self.control.stop_console(&repo)?;
            self.status_line = format!("stopped console for {}", repo.display());
        }
        self.console_repo = None;
        self.console_session = None;
        self.focus = Focus::Picker;
        Ok(())
    }

    pub fn handle_key(&mut self, key: KeyCode) -> Result<()> {
        if let Focus::Console = self.focus {
            match key {
                KeyCode::Esc => self.leave_console(),
                KeyCode::F(1) => self.stop_console()?,
                KeyCode::Enter => {
                    if let Some(session) = self.console_session.clone() {
                        let text = std::mem::take(&mut self.live_input);
                        self.control.send_text(&session, &text)?;
                        self.control.send_key(&session, "Enter")?;
                    }
                }
                KeyCode::Backspace => {
                    self.live_input.pop();
                }
                KeyCode::Char(c) => self.live_input.push(c),
                _ => {}
            }
            return Ok(());
        }

        if let Focus::Agent(AgentTab::Live) = self.focus {
            match key {
                KeyCode::Esc => self.back(),
                KeyCode::Tab => self.next_tab(),
                KeyCode::Enter => {
                    if let Some(d) = self.dispatches.get(self.dispatch_cursor) {
                        let text = std::mem::take(&mut self.live_input);
                        self.control.send_text(&d.session, &text)?;
                        self.control.send_key(&d.session, "Enter")?;
                    }
                }
                KeyCode::Backspace => {
                    self.live_input.pop();
                }
                KeyCode::Char(c) => self.live_input.push(c),
                _ => {}
            }
            return Ok(());
        }

        match (self.focus, key) {
            (_, KeyCode::Char('q')) => self.should_quit = true,
            (_, KeyCode::Tab) => self.next_tab(),
            (_, KeyCode::Down) => self.move_cursor(1),
            (_, KeyCode::Up) => self.move_cursor(-1),
            (Focus::Picker, KeyCode::Enter) => self.enter_roster()?,
            (Focus::Roster, KeyCode::Enter) => self.enter_agent(),
            (_, KeyCode::Esc) => self.back(),
            (Focus::Picker, KeyCode::Char('c')) => self.close_selected_run()?,
            (Focus::Picker, KeyCode::Char('v')) => self.open_console()?,
            _ => {}
        }
        Ok(())
    }

    fn base_branch(&self) -> String {
        self.selected_run()
            .and_then(|r| self.store.load_run(&r.id).ok())
            .map(|record| record.base_branch)
            .unwrap_or_else(|| "main".to_string())
    }

    pub fn render(&self, frame: &mut Frame) {
        match self.focus {
            Focus::Picker => self.render_picker(frame),
            Focus::Roster => self.render_roster(frame),
            Focus::Agent(tab) => self.render_agent(frame, tab),
            Focus::Console => self.render_console(frame),
        }
    }

    fn render_picker(&self, frame: &mut Frame) {
        let area = frame.area();
        let items: Vec<ListItem> = self
            .runs
            .iter()
            .enumerate()
            .map(|(i, r)| {
                let status = if r.open { "open" } else { "closed" };
                let marker = if r.needs_human { " NEEDS HUMAN" } else { "" };
                let exec = if r.exec.is_empty() { String::new() } else { format!(" {}", r.exec) };
                let label = format!("{} [{status}]{exec}{marker}", r.id);
                let style = if i == self.run_cursor {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                ListItem::new(label).style(style)
            })
            .collect();
        let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Runs"));
        frame.render_widget(list, area);
    }

    fn render_roster(&self, frame: &mut Frame) {
        let area = frame.area();
        let items: Vec<ListItem> = self
            .dispatches
            .iter()
            .enumerate()
            .map(|(i, d)| {
                let state = if d.alive {
                    "alive"
                } else if d.settled {
                    "settled"
                } else {
                    "stopped"
                };
                let exec = if d.exec.is_empty() { String::new() } else { format!(" {}", d.exec) };
                let label = format!("{} ({}) [{state}]{exec}", d.role, d.id);
                let style = if i == self.dispatch_cursor {
                    Style::default().add_modifier(Modifier::REVERSED)
                } else {
                    Style::default()
                };
                ListItem::new(label).style(style)
            })
            .collect();
        let title = self.selected_run().map(|r| r.id.clone()).unwrap_or_default();
        let list = List::new(items).block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Roster - {title}")),
        );
        frame.render_widget(list, area);
    }

    fn render_agent(&self, frame: &mut Frame, tab: AgentTab) {
        let area = frame.area();
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(3), Constraint::Min(1)])
            .split(area);

        let titles = [AgentTab::Live, AgentTab::Log, AgentTab::Diff, AgentTab::Checklist]
            .iter()
            .map(|t| Line::from(t.label()))
            .collect::<Vec<_>>();
        let agent_title = match self.dispatches.get(self.dispatch_cursor) {
            Some(d) if !d.exec.is_empty() => format!("Agent - {}", d.exec),
            _ => "Agent".to_string(),
        };
        let tabs = Tabs::new(titles)
            .select(tab.index())
            .block(Block::default().borders(Borders::ALL).title(agent_title));
        frame.render_widget(tabs, chunks[0]);

        let Some(dispatch) = self.dispatches.get(self.dispatch_cursor) else {
            frame.render_widget(Paragraph::new("no dispatch selected"), chunks[1]);
            return;
        };

        let body = match tab {
            AgentTab::Live => {
                let pane = self
                    .control
                    .capture_pane(&dispatch.session)
                    .unwrap_or_else(|e| format!("error: {e}"));
                format!("{pane}\n> {}", self.live_input)
            }
            AgentTab::Log => {
                let run_id = self.selected_run().map(|r| r.id.clone()).unwrap_or_default();
                let entries = self.log.read_run(&run_id).unwrap_or_default();
                entries
                    .iter()
                    .filter(|e| e.dispatch_id.as_deref() == Some(dispatch.id.as_str()))
                    .map(|e| format!("{} {} {}", e.timestamp, e.event, e.agent.clone().unwrap_or_default()))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            AgentTab::Diff => self
                .control
                .diff_worktree(Path::new(&dispatch.worktree), &self.base_branch())
                .unwrap_or_else(|e| format!("error: {e}")),
            AgentTab::Checklist => {
                let run_id = self.selected_run().map(|r| r.id.clone()).unwrap_or_default();
                let checklist = self.checklist.load(&run_id).unwrap_or_default();
                checklist
                    .items
                    .iter()
                    .zip(checklist.checked.iter())
                    .enumerate()
                    .map(|(i, (item, checked))| {
                        format!("[{}] {}. {item}", if *checked { "x" } else { " " }, i + 1)
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        };
        frame.render_widget(
            Paragraph::new(body).block(Block::default().borders(Borders::ALL).title(tab.label())),
            chunks[1],
        );
    }

    /// The console's view: typed into without attaching, the same as an agent's live tab
    /// (ticket Scope: "opening, leaving and stopping it from the live view").
    fn render_console(&self, frame: &mut Frame) {
        let area = frame.area();
        let session = self.console_session.clone().unwrap_or_default();
        let pane = self.control.capture_pane(&session).unwrap_or_else(|e| format!("error: {e}"));
        let body = format!("{pane}\n> {}", self.live_input);
        let title = self
            .console_repo
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_default();
        frame.render_widget(
            Paragraph::new(body).block(Block::default().borders(Borders::ALL).title(format!("Console - {title}"))),
            area,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event_log::{now_iso, LogEntry};
    use crate::store::{DispatchRecord, RunRecord};
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::cell::RefCell;
    use std::collections::HashMap;

    struct FixtureControl {
        alive: HashMap<String, bool>,
        panes: HashMap<String, String>,
        diffs: HashMap<String, String>,
        calls: RefCell<Vec<String>>,
        console_sessions: HashMap<String, String>,
    }

    impl Control for FixtureControl {
        fn session_alive(&self, session: &str) -> bool {
            *self.alive.get(session).unwrap_or(&false)
        }

        fn capture_pane(&self, session: &str) -> Result<String> {
            Ok(self.panes.get(session).cloned().unwrap_or_default())
        }

        fn send_text(&self, session: &str, text: &str) -> Result<()> {
            self.calls.borrow_mut().push(format!("send_text {session} {text}"));
            Ok(())
        }

        fn send_key(&self, session: &str, key: &str) -> Result<()> {
            self.calls.borrow_mut().push(format!("send_key {session} {key}"));
            Ok(())
        }

        fn resize(&self, session: &str, cols: u16, rows: u16) -> Result<()> {
            self.calls.borrow_mut().push(format!("resize {session} {cols}x{rows}"));
            Ok(())
        }

        fn diff_worktree(&self, worktree: &Path, _base_branch: &str) -> Result<String> {
            Ok(self
                .diffs
                .get(&worktree.to_string_lossy().to_string())
                .cloned()
                .unwrap_or_default())
        }

        fn close_run(&self, run_id: &str) -> Result<()> {
            self.calls.borrow_mut().push(format!("close_run {run_id}"));
            Ok(())
        }

        fn open_console(&self, repo: &Path, backend: &str) -> Result<String> {
            let repo = repo.to_string_lossy().to_string();
            self.calls.borrow_mut().push(format!("open_console {repo} {backend}"));
            Ok(self
                .console_sessions
                .get(&repo)
                .cloned()
                .unwrap_or_else(|| format!("console-{repo}")))
        }

        fn stop_console(&self, repo: &Path) -> Result<()> {
            self.calls
                .borrow_mut()
                .push(format!("stop_console {}", repo.to_string_lossy()));
            Ok(())
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        store: Store,
        log: EventLog,
        checklist: ChecklistStore,
        control: FixtureControl,
    }

    fn buffer_text(buffer: &ratatui::buffer::Buffer) -> String {
        let area = buffer.area;
        let mut out = String::new();
        for y in 0..area.height {
            for x in 0..area.width {
                if let Some(cell) = buffer.cell((area.x + x, area.y + y)) {
                    out.push_str(cell.symbol());
                }
            }
            out.push('\n');
        }
        out
    }

    fn build_fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::for_root(dir.path().join("runs"));
        let log = EventLog::for_dir(Some(dir.path().join("log")));
        let checklist = ChecklistStore::for_dir(Some(dir.path().join("checklists")));

        let open_run = RunRecord {
            id: "run-a-open".to_string(),
            name: "run-a-open".to_string(),
            repo: "/repo".to_string(),
            base_branch: "main".to_string(),
            backend: "claude".to_string(),
            created_at: now_iso(),
            closed_at: None,
            plugins: Vec::new(),
            meta_worktree: None,
            meta_dispatch_id: None,
            big_plan: Some("do the thing".to_string()),
        };
        store.create_run(&open_run).unwrap();

        let closed_run = RunRecord {
            id: "run-b-closed".to_string(),
            closed_at: Some(now_iso()),
            ..open_run.clone()
        };
        store.create_run(&closed_run).unwrap();

        log.record(&LogEntry {
            timestamp: now_iso(),
            run_id: "run-a-open".to_string(),
            dispatch_id: None,
            agent: None,
            event: events::NEEDS_HUMAN.to_string(),
            details: Some(serde_json::json!({"message": "which glaze?"})),
        })
        .unwrap();
        log.record(&LogEntry {
            timestamp: now_iso(),
            run_id: "run-a-open".to_string(),
            dispatch_id: None,
            agent: None,
            event: events::EXEC_PROFILE_SELECTED.to_string(),
            details: Some(serde_json::json!({"profile": "gb10", "source": "--exec-profile"})),
        })
        .unwrap();
        log.record(&LogEntry {
            timestamp: now_iso(),
            run_id: "run-b-closed".to_string(),
            dispatch_id: None,
            agent: None,
            event: events::EXEC_PROFILE_NONE.to_string(),
            details: Some(serde_json::json!({"profile": null, "source": "none configured"})),
        })
        .unwrap();
        log.record(&LogEntry {
            timestamp: now_iso(),
            run_id: "run-a-open".to_string(),
            dispatch_id: Some("dispatch-1".to_string()),
            agent: Some("worker".to_string()),
            event: events::AGENT_ENTER.to_string(),
            details: None,
        })
        .unwrap();

        let dispatch = DispatchRecord {
            id: "dispatch-1".to_string(),
            run_id: "run-a-open".to_string(),
            role: "worker".to_string(),
            backend: "claude".to_string(),
            worktree: "/repo.oat-run-a-open-worker".to_string(),
            branch: "oat/run-a-open/worker".to_string(),
            created_at: now_iso(),
            env_id: Some("oat-env-worker".to_string()),
            image_id: Some("sha256:abc".to_string()),
            env_skipped: None,
            settled: None,
            report: None,
            released_at: None,
        };
        store.create_dispatch(&dispatch).unwrap();
        store
            .create_dispatch(&DispatchRecord {
                id: "dispatch-2".to_string(),
                role: "reviewer".to_string(),
                env_id: None,
                image_id: None,
                env_skipped: Some("this Run has no execution profile".to_string()),
                ..dispatch.clone()
            })
            .unwrap();

        let hid = crate::event_log::hash_id("worker", "run-a-open", "dispatch-1");
        let session = crate::session::tmux::Tmux::session_name(&hid);

        checklist
            .replace_items("run-a-open", vec!["write code".to_string(), "run tests".to_string()])
            .unwrap();
        checklist.set_checked("run-a-open", 1, true).unwrap();

        let mut alive = HashMap::new();
        alive.insert(session.clone(), true);
        let mut panes = HashMap::new();
        panes.insert(session.clone(), "$ cargo test\nrunning 3 tests ... ok".to_string());
        let mut diffs = HashMap::new();
        diffs.insert(
            "/repo.oat-run-a-open-worker".to_string(),
            "diff --git a/x b/x\n+added line".to_string(),
        );
        let console_session = "console-repo-session".to_string();
        panes.insert(console_session.clone(), "$ oat-agents log runs --repo /repo".to_string());
        let mut console_sessions = HashMap::new();
        console_sessions.insert("/repo".to_string(), console_session);

        Fixture {
            _dir: dir,
            store,
            log,
            checklist,
            control: FixtureControl {
                alive,
                panes,
                diffs,
                calls: RefCell::new(Vec::new()),
                console_sessions,
            },
        }
    }

    fn draw(app: &App) -> String {
        let backend = TestBackend::new(60, 20);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|frame| app.render(frame)).unwrap();
        buffer_text(terminal.backend().buffer())
    }

    #[test]
    fn picker_lists_runs_and_marks_the_open_needs_human_run() {
        let fx = build_fixture();
        let app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        let text = draw(&app);
        assert!(text.contains("run-a-open"), "{text}");
        assert!(text.contains("run-b-closed"), "{text}");
        assert!(text.contains("NEEDS HUMAN"), "{text}");
        assert!(text.contains("[closed]"), "{text}");
    }

    #[test]
    fn picker_shows_where_each_runs_commands_run() {
        let fx = build_fixture();
        let app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        let text = draw(&app);
        assert!(text.contains("run-a-open [open] pod:gb10"), "{text}");
        assert!(text.contains("run-b-closed [closed] host"), "{text}");
    }

    #[test]
    fn roster_and_agent_page_show_each_dispatchs_pod_or_its_absence() {
        let fx = build_fixture();
        let mut app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        app.enter_roster().unwrap();
        let text = draw(&app);
        assert!(text.contains("pod:oat-env-worker"), "{text}");
        assert!(text.contains("HOST (no pod)"), "{text}");
        app.enter_agent();
        let text = draw(&app);
        assert!(text.contains("Agent - pod:oat-env-worker"), "{text}");
    }

    #[test]
    fn roster_lists_the_selected_runs_dispatches_with_liveness() {
        let fx = build_fixture();
        let mut app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        assert_eq!(app.runs[app.run_cursor].id, "run-a-open");
        app.enter_roster().unwrap();
        let text = draw(&app);
        assert!(text.contains("worker"), "{text}");
        assert!(text.contains("dispatch-1"), "{text}");
        assert!(text.contains("[alive]"), "{text}");
    }

    #[test]
    fn live_tab_shows_the_captured_pane_and_types_without_attaching() {
        let fx = build_fixture();
        let mut app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        app.set_viewport(60, 20);
        app.enter_roster().unwrap();
        app.enter_agent();
        let text = draw(&app);
        assert!(text.contains("running 3 tests"), "{text}");

        for c in "yes".chars() {
            app.handle_key(KeyCode::Char(c)).unwrap();
        }
        app.handle_key(KeyCode::Enter).unwrap();

        let calls = fx.control.calls.borrow();
        assert!(calls.iter().any(|c| c.contains("send_text") && c.contains("yes")), "{calls:?}");
        assert!(calls.iter().any(|c| c.contains("send_key") && c.contains("Enter")), "{calls:?}");
        assert!(calls.iter().any(|c| c.contains("resize") && c.contains("60x20")), "{calls:?}");
    }

    #[test]
    fn log_tab_shows_only_the_selected_dispatchs_log_entries() {
        let fx = build_fixture();
        // A run-level entry with no agent, and a second Dispatch of the *same* role
        // ("worker") — the shape a correction round creates in one Run — must not leak into
        // the selected Dispatch's Log tab (ticket Scope: "its log", not the whole Run's log,
        // nor every Dispatch sharing its role).
        fx.log
            .record(&LogEntry {
                timestamp: now_iso(),
                run_id: "run-a-open".to_string(),
                dispatch_id: Some("dispatch-2".to_string()),
                agent: Some("worker".to_string()),
                event: events::AGENT_EXIT.to_string(),
                details: None,
            })
            .unwrap();

        let mut app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        app.enter_roster().unwrap();
        app.enter_agent();
        app.handle_key(KeyCode::Tab).unwrap();
        let text = draw(&app);
        assert!(text.contains(events::AGENT_ENTER), "{text}");
        assert!(text.contains("worker"), "{text}");
        assert!(!text.contains(events::NEEDS_HUMAN), "{text} (a Run-level entry with no agent leaked in)");
        assert!(
            !text.contains(events::AGENT_EXIT),
            "{text} (another Dispatch of the same role leaked in)"
        );
    }

    #[test]
    fn diff_tab_shows_the_worktree_diff() {
        let fx = build_fixture();
        let mut app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        app.enter_roster().unwrap();
        app.enter_agent();
        app.handle_key(KeyCode::Tab).unwrap();
        app.handle_key(KeyCode::Tab).unwrap();
        let text = draw(&app);
        assert!(text.contains("added line"), "{text}");
    }

    #[test]
    fn checklist_tab_shows_items_and_checked_state() {
        let fx = build_fixture();
        let mut app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        app.enter_roster().unwrap();
        app.enter_agent();
        app.handle_key(KeyCode::Tab).unwrap();
        app.handle_key(KeyCode::Tab).unwrap();
        app.handle_key(KeyCode::Tab).unwrap();
        let text = draw(&app);
        assert!(text.contains("write code"), "{text}");
        assert!(text.contains("run tests"), "{text}");
        assert!(text.contains("[x]"), "{text}");
    }

    #[test]
    fn closing_a_run_goes_through_control_and_returns_to_the_picker() {
        let fx = build_fixture();
        let mut app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        app.handle_key(KeyCode::Char('c')).unwrap();
        assert_eq!(app.focus, Focus::Picker);
        let calls = fx.control.calls.borrow();
        assert!(calls.iter().any(|c| c.contains("close_run run-a-open")), "{calls:?}");
    }

    #[test]
    fn opening_the_console_shows_its_pane_and_types_into_it_without_attaching() {
        let fx = build_fixture();
        let mut app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        app.set_viewport(60, 20);

        app.handle_key(KeyCode::Char('v')).unwrap();
        assert_eq!(app.focus, Focus::Console);

        let text = draw(&app);
        assert!(text.contains("oat-agents log runs"), "{text}");

        for c in "hi".chars() {
            app.handle_key(KeyCode::Char(c)).unwrap();
        }
        app.handle_key(KeyCode::Enter).unwrap();

        let calls = fx.control.calls.borrow();
        assert!(calls.iter().any(|c| c.contains("open_console /repo claude")), "{calls:?}");
        assert!(
            calls.iter().any(|c| c.contains("resize console-repo-session 60x20")),
            "{calls:?}"
        );
        assert!(calls.iter().any(|c| c.contains("send_text") && c.contains("hi")), "{calls:?}");
        assert!(calls.iter().any(|c| c.contains("send_key") && c.contains("Enter")), "{calls:?}");
    }

    #[test]
    fn leaving_the_console_returns_to_the_picker_without_stopping_it() {
        let fx = build_fixture();
        let mut app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        app.handle_key(KeyCode::Char('v')).unwrap();
        assert_eq!(app.focus, Focus::Console);

        app.handle_key(KeyCode::Esc).unwrap();

        assert_eq!(app.focus, Focus::Picker);
        assert!(app.console_session.is_some(), "leaving must not clear the open session");
        let calls = fx.control.calls.borrow();
        assert!(!calls.iter().any(|c| c.contains("stop_console")), "{calls:?}");
    }

    #[test]
    fn stopping_the_console_goes_through_control_and_returns_to_the_picker() {
        let fx = build_fixture();
        let mut app = App::new(&fx.store, &fx.log, &fx.checklist, &fx.control).unwrap();
        app.handle_key(KeyCode::Char('v')).unwrap();
        assert_eq!(app.focus, Focus::Console);

        app.handle_key(KeyCode::F(1)).unwrap();

        assert_eq!(app.focus, Focus::Picker);
        assert!(app.console_session.is_none(), "stopping must clear the open session");
        let calls = fx.control.calls.borrow();
        assert!(calls.iter().any(|c| c.contains("stop_console /repo")), "{calls:?}");
    }
}
