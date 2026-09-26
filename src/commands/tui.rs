use crate::checklist::ChecklistStore;
use crate::environment::Environment;
use crate::event_log::EventLog;
use crate::store::Store;
use crate::tui::{App, RealControl};
use anyhow::Result;
use crossterm::event::{self, Event, KeyEventKind};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::{Backend, CrosstermBackend};
use ratatui::Terminal;
use serde_json::{json, Value};
use std::io::stdout;
use std::time::Duration;

/// The real terminal loop. Everything it decides — what to render, what a key does — lives in
/// `tui::App`, tested on its own against recorded Runs; this function only owns the terminal
/// and the event source, neither of which a test can usefully fake.
pub fn run(env: &dyn Environment) -> Result<Value> {
    let store = Store::open(env)?;
    let log = EventLog::open(env);
    let checklist = ChecklistStore::open(env);
    let control = RealControl { env };
    let mut app = App::new(&store, &log, &checklist, &control)?;

    enable_raw_mode()?;
    let mut out = stdout();
    out.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(out);
    let mut terminal = Terminal::new(backend)?;

    let result = event_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    terminal.backend_mut().execute(LeaveAlternateScreen)?;

    result?;
    Ok(json!({"exited": true}))
}

fn event_loop<B: Backend>(terminal: &mut Terminal<B>, app: &mut App) -> Result<()> {
    while !app.should_quit {
        let size = terminal.size()?;
        app.set_viewport(size.width, size.height);
        terminal.draw(|frame| app.render(frame))?;
        if event::poll(Duration::from_millis(200))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    app.handle_key(key.code)?;
                }
            }
        }
    }
    Ok(())
}
