use super::*;
use crate::event_log::{LogEntry, now_iso};
use crate::store::{DispatchRecord, RunRecord};
use ratatui::backend::TestBackend;
use std::cell::RefCell;
use std::collections::HashSet;

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn ctrl(c: char) -> KeyEvent {
    KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
}

/// A stand-in for the machine: sessions it calls alive, screens and diffs it hands back, and
/// every call that would have changed something, recorded.
#[derive(Default)]
struct RecordingControl {
    alive: HashSet<String>,
    screens: HashMap<String, String>,
    diffs: HashMap<String, String>,
    consoles: HashMap<PathBuf, String>,
    calls: RefCell<Vec<String>>,
}

impl RecordingControl {
    fn calls(&self) -> Vec<String> {
        self.calls.borrow().clone()
    }

    fn record(&self, call: String) {
        self.calls.borrow_mut().push(call);
    }
}

impl Control for RecordingControl {
    fn close_run(&self, run_id: &str) -> Result<String> {
        self.record(format!("close_run {run_id}"));
        Ok("closed".to_string())
    }

    fn clean_run(&self, run_id: &str) -> Result<String> {
        self.record(format!("clean_run {run_id}"));
        Ok("removed 2 worktrees".to_string())
    }

    fn type_input(&self, session: &str, input: &live::LiveInput) -> Result<()> {
        self.record(format!("type {session} {input:?}"));
        Ok(())
    }

    fn cursor(&self, _session: &str) -> Option<(u16, u16)> {
        None
    }

    fn screen_mode(&self, _session: &str) -> Option<live::ScreenMode> {
        None
    }

    fn capture(&self, session: &str) -> Result<String> {
        Ok(self.screens.get(session).cloned().unwrap_or_default())
    }

    fn capture_history(&self, session: &str) -> Result<String> {
        self.capture(session)
    }

    fn resize(&self, session: &str, cols: u16, rows: u16) -> Result<()> {
        self.record(format!("resize {session} {cols}x{rows}"));
        Ok(())
    }

    fn is_alive(&self, session: &str) -> bool {
        self.alive.contains(session)
    }

    fn diff(&self, worktree: &Path, _base: &str) -> Result<DiffReport> {
        let text = self
            .diffs
            .get(&worktree.display().to_string())
            .cloned()
            .unwrap_or_default();
        Ok(DiffReport {
            added: text.lines().filter(|line| line.starts_with('+')).count() as u32,
            removed: 0,
            text,
        })
    }

    fn diff_stat(&self, worktree: &Path, base: &str) -> Result<(u32, u32)> {
        self.diff(worktree, base).map(|report| (report.added, report.removed))
    }

    fn console_live(&self, repo: &Path) -> bool {
        self.consoles.contains_key(repo)
    }

    fn console(&self, repo: &Path) -> Result<ConsoleHandle> {
        self.record(format!("console {}", repo.display()));
        Ok(ConsoleHandle {
            repo: repo.to_path_buf(),
            backend: "claude".to_string(),
            started_at: now_iso(),
            session: self
                .consoles
                .get(repo)
                .cloned()
                .unwrap_or_else(|| "oat_console-repo".to_string()),
            dir: None,
        })
    }

    fn stop_console(&self, repo: &Path) -> Result<String> {
        self.record(format!("stop_console {}", repo.display()));
        Ok("stopped".to_string())
    }
}

struct Fixture {
    _dir: tempfile::TempDir,
    store: Store,
    log: EventLog,
    checklist: ChecklistStore,
}

const OPEN_RUN: &str = "run-a-open";
const CLOSED_RUN: &str = "run-b-closed";

fn entry(run_id: &str, dispatch_id: Option<&str>, agent: Option<&str>, event: &str, details: Value) -> LogEntry {
    LogEntry {
        timestamp: now_iso(),
        run_id: run_id.to_string(),
        dispatch_id: dispatch_id.map(str::to_string),
        agent: agent.map(str::to_string),
        event: event.to_string(),
        details: Some(details).filter(|details| !details.is_null()),
    }
}

/// Two Runs: an open one waiting on a person, whose coordinator, worker (in a pod) and
/// reviewer (on the host though it asked for a pod, and already settled) are on record; and
/// a closed one that ran on the host.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::for_root(dir.path().join("runs"));
    let log = EventLog::for_dir(Some(dir.path().join("log")));
    let checklist = ChecklistStore::for_dir(Some(dir.path().join("checklists")));

    let open = RunRecord {
        id: OPEN_RUN.to_string(),
        name: "glaze".to_string(),
        repo: "/repos/pottery".to_string(),
        base_branch: "main".to_string(),
        backend: "claude".to_string(),
        created_at: now_iso(),
        closed_at: None,
        plugins: Vec::new(),
        meta_worktree: None,
        meta_dispatch_id: Some("dispatch-0".to_string()),
        big_plan: Some("# Glaze every pot\n\nThen fire the kiln.".to_string()),
    };
    store.create_run(&open).unwrap();
    store
        .create_run(&RunRecord {
            id: CLOSED_RUN.to_string(),
            closed_at: Some(now_iso()),
            big_plan: Some("Sweep the studio".to_string()),
            ..open.clone()
        })
        .unwrap();

    let meta = DispatchRecord {
        id: "dispatch-0".to_string(),
        run_id: OPEN_RUN.to_string(),
        role: "oat-meta".to_string(),
        backend: "claude".to_string(),
        worktree: "/repos/pottery.oat-glaze-meta".to_string(),
        branch: "oat/glaze/meta".to_string(),
        created_at: "2026-09-26T01:00:00.000000000Z".to_string(),
        env_id: None,
        image_id: None,
        env_skipped: None,
        settled: None,
        report: None,
        released_at: None,
    };
    store.create_dispatch(&meta).unwrap();
    store
        .create_dispatch(&DispatchRecord {
            id: "dispatch-1".to_string(),
            role: "worker".to_string(),
            worktree: "/repos/pottery.oat-glaze-worker".to_string(),
            branch: "oat/glaze/worker".to_string(),
            created_at: "2026-09-26T01:01:00.000000000Z".to_string(),
            env_id: Some("oat-env-worker".to_string()),
            image_id: Some("sha256:0123456789abcdef".to_string()),
            ..meta.clone()
        })
        .unwrap();
    store
        .create_dispatch(&DispatchRecord {
            id: "dispatch-2".to_string(),
            role: "reviewer".to_string(),
            worktree: "/repos/pottery.oat-glaze-reviewer".to_string(),
            branch: "oat/glaze/reviewer".to_string(),
            created_at: "2026-09-26T01:02:00.000000000Z".to_string(),
            env_skipped: Some("this Run has no execution profile".to_string()),
            settled: Some(crate::store::Settlement::Succeeded),
            ..meta.clone()
        })
        .unwrap();

    let details = |value: Value| value;
    for record in [
        entry(OPEN_RUN, None, None, events::RUN_CREATED, details(serde_json::json!({"big_plan": "Glaze every pot"}))),
        entry(OPEN_RUN, None, None, events::EXEC_PROFILE_SELECTED, serde_json::json!({"profile": "gb10"})),
        entry(OPEN_RUN, Some("dispatch-0"), Some("oat-meta"), events::AGENT_ENTER, serde_json::json!({"backend": "claude"})),
        entry(OPEN_RUN, Some("dispatch-1"), Some("worker"), events::AGENT_ENTER, serde_json::json!({"backend": "claude"})),
        entry(OPEN_RUN, Some("dispatch-2"), Some("reviewer"), events::AGENT_ENTER, serde_json::json!({"backend": "claude"})),
        entry(OPEN_RUN, Some("dispatch-2"), Some("reviewer"), events::AGENT_EXIT, serde_json::json!({"outcome": "succeeded"})),
        entry(OPEN_RUN, None, None, events::NEEDS_HUMAN, serde_json::json!({"message": "which glaze?"})),
        entry(CLOSED_RUN, None, None, events::EXEC_PROFILE_NONE, serde_json::json!({"profile": null})),
    ] {
        log.record(&record).unwrap();
    }

    checklist
        .replace_items(OPEN_RUN, vec!["glaze pots".to_string(), "fire kiln".to_string()])
        .unwrap();
    checklist.set_checked(OPEN_RUN, 1, true).unwrap();

    Fixture {
        _dir: dir,
        store,
        log,
        checklist,
    }
}

impl Fixture {
    fn snapshot(&self, run_id: &str) -> RunSnapshot {
        RunSnapshot::read(&self.store, &self.log, run_id).unwrap()
    }

    fn state(&self) -> TuiState {
        TuiState::with_projects(self.snapshot(OPEN_RUN), None)
    }

    fn picker(&self) -> Picker {
        Picker::new(model::run_summaries(&self.store, &self.log))
    }

    fn session_of(&self, role: &str) -> String {
        let snapshot = self.snapshot(OPEN_RUN);
        let agent = snapshot
            .agents()
            .into_iter()
            .find(|agent| agent.role.as_deref() == Some(role))
            .unwrap();
        agent.terminal_handle.unwrap()
    }
}

fn screen(width: u16, height: u16, paint: impl FnOnce(&mut Frame)) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(paint).unwrap();
    let buffer = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn select(state: &mut TuiState, role: &str) {
    let index = state
        .agents()
        .iter()
        .position(|agent| agent.role.as_deref() == Some(role))
        .unwrap();
    state.selected = index + 1;
}

#[test]
fn the_picker_lists_every_run_with_its_title_its_pod_and_who_it_waits_on() {
    let fx = fixture();
    let picker = fx.picker();
    let text = screen(160, 12, |frame| draw_runs(frame, &picker, fx.log.dir(), &HashMap::new()));
    assert!(text.contains(" oat-agents "), "{text}");
    assert!(text.contains("2 runs (1 running)"), "{text}");
    assert!(text.contains(OPEN_RUN) && text.contains(CLOSED_RUN), "{text}");
    assert!(text.contains("pottery"), "the repo's folder names the Run:\n{text}");
    assert!(
        text.contains("which glaze?") && !text.contains("Glaze every pot"),
        "an open question outranks the title:\n{text}"
    );
    assert!(text.contains("Sweep the studio"), "the Big Plan's first line is the title:\n{text}");
    assert!(text.contains("pod:gb10"), "{text}");
    assert!(text.contains("host"), "{text}");
    assert!(text.contains("enter open"), "{text}");
    // Running first: the open Run heads the list.
    assert!(text.find(OPEN_RUN).unwrap() < text.find(CLOSED_RUN).unwrap(), "{text}");
}

#[test]
fn the_picker_opens_a_run_on_enter_or_the_right_arrow() {
    let fx = fixture();
    let control = RecordingControl::default();
    let mut picker = fx.picker();
    assert_eq!(picker.on_key(key(KeyCode::Enter), &control), PickerAction::Open(OPEN_RUN.into()));
    assert_eq!(picker.on_key(key(KeyCode::Right), &control), PickerAction::Open(OPEN_RUN.into()));
    picker.on_key(key(KeyCode::Down), &control);
    assert_eq!(picker.on_key(key(KeyCode::Enter), &control), PickerAction::Open(CLOSED_RUN.into()));
    assert_eq!(picker.on_key(key(KeyCode::Char('q')), &control), PickerAction::Quit);
}

#[test]
fn closing_a_run_from_the_picker_takes_a_deliberate_yes() {
    let fx = fixture();
    let control = RecordingControl::default();
    let mut picker = fx.picker();

    picker.on_key(key(KeyCode::Char('x')), &control);
    assert!(picker.confirming().is_some());
    picker.on_key(key(KeyCode::Char('n')), &control);
    assert_eq!(picker.notice(), Some("run-a-open left running"));
    assert!(control.calls().is_empty());

    picker.on_key(key(KeyCode::Char('x')), &control);
    picker.on_key(key(KeyCode::Char('y')), &control);
    assert_eq!(control.calls(), vec!["close_run run-a-open"]);
    assert_eq!(picker.notice(), Some("run-a-open closed"));
}

#[test]
fn a_finished_run_can_be_cleaned_from_the_picker_and_an_open_one_cannot() {
    let fx = fixture();
    let control = RecordingControl::default();
    let mut picker = fx.picker();

    picker.on_key(key(KeyCode::Char('c')), &control);
    assert_eq!(picker.notice(), Some("run-a-open is still running; close it first"));
    picker.on_key(key(KeyCode::Down), &control);
    picker.on_key(key(KeyCode::Char('x')), &control);
    assert_eq!(picker.notice(), Some("run-b-closed has nothing running"));
    picker.on_key(key(KeyCode::Char('c')), &control);
    assert_eq!(control.calls(), vec!["clean_run run-b-closed"]);
}

#[test]
fn the_coordinator_is_named_under_meta_as_its_launch_named_it() {
    let fx = fixture();
    let state = fx.state();
    let meta = state.agents().iter().find(|agent| agent.is_meta()).unwrap();
    assert_eq!(meta.hash_id, crate::event_log::hash_id("meta", "glaze", "dispatch-0"));
    assert_eq!(
        meta.terminal_handle.as_deref(),
        Some(crate::session::tmux::Tmux::session_name(&meta.hash_id).as_str())
    );
}

#[test]
fn the_run_view_names_every_dispatch_where_it_runs_and_the_keys() {
    let fx = fixture();
    let mut state = fx.state();
    let text = screen(160, 24, |frame| draw(frame, &mut state));
    assert!(text.contains(" run-a-open "), "{text}");
    assert!(text.contains("3 agents (2 active)"), "{text}");
    for agent in state.agents() {
        assert!(text.contains(&agent.hash_id), "{} missing:\n{text}", agent.hash_id);
    }
    assert!(text.contains("pod:oat-env-worker"), "{text}");
    assert!(text.contains("HOST (no pod)"), "{text}");
    assert!(text.contains("succeeded at"), "a finished agent says how it ended:\n{text}");
    assert!(text.contains("finished"), "the rule separates live from finished:\n{text}");
    assert!(text.contains(WAITING_MARK), "the coordinator that asked is marked:\n{text}");
    assert!(text.contains(" timeline "), "{text}");
    assert!(text.contains("enter open live"), "{text}");
    assert!(text.contains("ctrl+\\ console"), "{text}");
    assert!(text.contains("ctrl+l checklist"), "{text}");
}

#[test]
fn selecting_a_dispatch_narrows_the_timeline_to_its_own_entries() {
    let fx = fixture();
    // A second Dispatch of the same role must not leak into the first one's timeline.
    fx.log
        .record(&entry(OPEN_RUN, Some("dispatch-3"), Some("worker"), events::AGENT_EXIT, Value::Null))
        .unwrap();
    let mut state = fx.state();
    assert_eq!(state.visible_events().len(), 8, "all agents shows every entry");
    select(&mut state, "worker");
    let events: Vec<String> = state
        .visible_events()
        .iter()
        .filter_map(|row| string_field(row, "event"))
        .collect();
    assert_eq!(events, vec![events::AGENT_ENTER.to_string()]);
}

#[test]
fn the_event_filter_cycles_through_all_lifecycle_and_decisions() {
    let fx = fixture();
    let control = RecordingControl::default();
    let mut state = fx.state();
    state.on_key_with(key(KeyCode::Char('e')), &control);
    assert_eq!(state.filter(), EventFilter::Lifecycle);
    assert_eq!(state.visible_events().len(), 4);
    state.on_key_with(key(KeyCode::Char('e')), &control);
    assert_eq!(state.filter(), EventFilter::Decisions);
    assert_eq!(state.visible_events().len(), 1, "the question to a person is a decision point");
    state.on_key_with(key(KeyCode::Char('e')), &control);
    assert_eq!(state.filter(), EventFilter::All);
}

#[test]
fn a_live_agents_page_opens_on_its_screen_and_hands_it_every_key() {
    let fx = fixture();
    let session = fx.session_of("worker");
    let mut control = RecordingControl::default();
    control.alive.insert(session.clone());
    control
        .screens
        .insert(session.clone(), "$ cargo test\nrunning 3 tests ... ok".to_string());
    let mut state = fx.state();
    state.refresh_sessions(&control);
    select(&mut state, "worker");

    state.on_key_with(key(KeyCode::Enter), &control);
    assert_eq!(state.view(), View::Detail);
    assert_eq!(state.tab(), Tab::Live);
    assert!(state.typing(), "the live tab starts typing at the agent");
    state.refresh_preview(&control);

    let text = screen(120, 30, |frame| draw(frame, &mut state));
    assert!(text.contains("running 3 tests"), "{text}");
    assert!(text.contains("live · typing (ctrl+] leaves)"), "{text}");
    state.fit_preview(&control);

    state.on_key_with(key(KeyCode::Char('q')), &control);
    state.on_key_with(key(KeyCode::Enter), &control);
    assert_eq!(state.view(), View::Detail, "q is the agent's while typing");
    let calls = control.calls();
    assert!(calls.contains(&format!("type {session} Text(\"q\")")), "{calls:?}");
    assert!(calls.contains(&format!("type {session} Key(\"Enter\")")), "{calls:?}");
    assert!(calls.iter().any(|call| call.starts_with(&format!("resize {session}"))), "{calls:?}");

    state.on_key_with(ctrl(']'), &control);
    assert!(!state.typing());
    state.on_key_with(key(KeyCode::Tab), &control);
    assert_eq!(state.tab(), Tab::Diff);
    state.on_key_with(key(KeyCode::Tab), &control);
    assert_eq!(state.tab(), Tab::Log);
    state.on_key_with(key(KeyCode::Esc), &control);
    assert_eq!(state.view(), View::Overview);
}

#[test]
fn the_diff_tab_shows_what_the_worktree_changed() {
    let fx = fixture();
    let session = fx.session_of("worker");
    let mut control = RecordingControl::default();
    control.alive.insert(session);
    control.diffs.insert(
        "/repos/pottery.oat-glaze-worker".to_string(),
        "diff --git a/x b/x\n+added line".to_string(),
    );
    let mut state = fx.state();
    select(&mut state, "worker");
    state.open_detail(&control);
    state.on_key_with(ctrl(']'), &control);
    state.on_key_with(key(KeyCode::Tab), &control);
    let text = screen(120, 30, |frame| draw(frame, &mut state));
    assert!(text.contains("added line"), "{text}");
    assert!(text.contains("since main"), "{text}");
}

#[test]
fn a_finished_agent_has_only_its_log_which_says_where_it_ran() {
    let fx = fixture();
    let control = RecordingControl::default();
    let mut state = fx.state();
    select(&mut state, "reviewer");
    state.open_detail(&control);
    assert_eq!(state.tabs(), &[Tab::Log]);
    assert_eq!(state.tab(), Tab::Log);
    let text = screen(120, 30, |frame| draw(frame, &mut state));
    assert!(text.contains("HOST (no pod): this Run has no execution profile"), "{text}");
    assert!(text.contains("outcome succeeded"), "{text}");
    assert!(text.contains("agent_exit"), "without a transcript the log's own record shows:\n{text}");

    let mut state = fx.state();
    select(&mut state, "worker");
    state.open_detail(&control);
    let text = screen(120, 30, |frame| draw(frame, &mut state));
    assert!(text.contains("env pod:oat-env-worker   image 0123456789ab"), "{text}");
}

#[test]
fn closing_a_run_from_its_view_takes_a_deliberate_yes() {
    let fx = fixture();
    let control = RecordingControl::default();
    let mut state = fx.state();
    assert_eq!(state.on_key_with(key(KeyCode::Char('x')), &control), Action::Continue);
    assert!(state.closing());
    let text = screen(160, 24, |frame| draw(frame, &mut state));
    assert!(text.contains("close run-a-open and stop 2 live agents"), "{text}");
    assert_eq!(
        state.on_key_with(key(KeyCode::Char('y')), &control),
        Action::CloseRun(OPEN_RUN.to_string())
    );
    state.on_key_with(key(KeyCode::Char('x')), &control);
    assert_eq!(state.on_key_with(key(KeyCode::Char('n')), &control), Action::Continue);
    assert_eq!(state.notice(), Some("left running"));
}

#[test]
fn the_checklist_panel_shows_the_runs_items_and_how_many_are_done() {
    let fx = fixture();
    let mut panel = ChecklistPanel::new();
    panel.open_for(OPEN_RUN.to_string());
    panel.refresh(Some(&fx.checklist));
    let text = screen(100, 30, |frame| checklist_panel::draw(frame, &panel));
    assert!(text.contains("checklist · 1/2"), "{text}");
    assert!(text.contains("[x] 1. glaze pots"), "{text}");
    assert!(text.contains("[ ] 2. fire kiln"), "{text}");
}

#[test]
fn the_console_window_has_a_screen_and_a_log_and_nothing_to_close_but_itself() {
    let control = RecordingControl::default();
    let handle = control.console(Path::new("/repos/pottery")).unwrap();
    let mut state = console_state(&handle, None);
    assert_eq!(state.agents().len(), 1);
    state.selected = 1;
    assert_eq!(state.tabs(), &[Tab::Live, Tab::Log]);
    assert_eq!(state.agents()[0].hash_id, "console-pottery");

    let text = screen(160, 24, |frame| draw(frame, &mut state));
    assert!(text.contains("x stop console"), "{text}");
    assert!(!text.contains("checklist"), "the console has no Run, so no checklist:\n{text}");

    state.on_key_with(key(KeyCode::Char('x')), &control);
    assert_eq!(state.on_key_with(key(KeyCode::Char('y')), &control), Action::StopConsole);
    state.on_key_with(key(KeyCode::Char('x')), &control);
    state.on_key_with(key(KeyCode::Esc), &control);
    assert_eq!(state.notice(), Some("console left running"));
}

#[test]
fn the_console_key_opens_the_repository_the_view_was_started_in() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().canonicalize().unwrap();
    assert!(std::process::Command::new("git").arg("init").arg("-q").arg(&repo).status().unwrap().success());
    let inside = repo.join("src").join("deep");
    std::fs::create_dir_all(&inside).unwrap();
    assert_eq!(console_target(&inside), repo);

    let outside = tempfile::tempdir().unwrap();
    let plain = outside.path().canonicalize().unwrap();
    assert_eq!(console_target(&plain), plain, "outside a repository it is the directory itself");

    assert!(is_console_toggle(&ctrl('\\')));
    assert!(is_console_toggle(&key(KeyCode::F(2))));
}
