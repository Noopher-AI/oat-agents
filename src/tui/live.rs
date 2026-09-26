//! Typing at an agent from its live screen.
//!
//! The live tab is the agent's terminal: every key pressed there is handed
//! to its tmux session as the key it was, and the next capture shows what
//! the agent made of it. Nothing is buffered here, so what is on screen is
//! always what the agent has.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// What one key or one paste becomes on its way into the session.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LiveInput {
    /// Characters typed as they are.
    Text(String),
    /// One key by its tmux name: `Enter`, `C-c`, `S-Up`.
    Key(String),
    /// A block of text pasted in one piece.
    Paste(String),
}

/// How the program in a pane is drawing, which decides who scrolls it.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ScreenMode {
    /// It drew on the alternate screen, as a full-screen program does, so
    /// tmux keeps no history for it: the program keeps its own.
    pub alternate: bool,
    /// It asked for mouse reports, so it reads the wheel itself.
    pub mouse: bool,
}

/// One notch of the wheel as an SGR mouse report at a point in the pane,
/// 1-based, typed at the program the way its terminal would send it.
pub fn wheel(up: bool, column: u16, row: u16) -> LiveInput {
    LiveInput::Text(format!(
        "\x1b[<{};{column};{row}M",
        if up { 64 } else { 65 }
    ))
}

/// Leaves the live screen. Ctrl+], the key telnet and ssh already use to
/// step out of a remote terminal, and one no agent or shell binds. A
/// terminal without the enhanced keyboard reports it as Ctrl+5.
pub fn is_leave(key: &KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char(']') | KeyCode::Char('5'))
}

/// The tmux input a key stands for, or `None` for a key tmux has no name
/// for.
pub fn input_for(key: &KeyEvent) -> Option<LiveInput> {
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    let shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let named = |name: &str| {
        let mut prefix = String::new();
        if ctrl {
            prefix.push_str("C-");
        }
        if alt {
            prefix.push_str("M-");
        }
        if shift {
            prefix.push_str("S-");
        }
        Some(LiveInput::Key(format!("{prefix}{name}")))
    };
    match key.code {
        KeyCode::Char(c) if ctrl || alt => {
            let c = if c == ' ' {
                "Space".to_owned()
            } else {
                c.to_ascii_lowercase().to_string()
            };
            let mut name = String::new();
            if ctrl {
                name.push_str("C-");
            }
            if alt {
                name.push_str("M-");
            }
            name.push_str(&c);
            Some(LiveInput::Key(name))
        }
        KeyCode::Char(c) => Some(LiveInput::Text(c.to_string())),
        // A newline inside a message rather than sending it: the agents
        // read Alt+Enter that way, and tmux has no name for Shift+Enter.
        KeyCode::Enter if alt || shift => Some(LiveInput::Key("M-Enter".to_owned())),
        KeyCode::Enter => Some(LiveInput::Key("Enter".to_owned())),
        KeyCode::Esc => Some(LiveInput::Key("Escape".to_owned())),
        KeyCode::Backspace => Some(LiveInput::Key("BSpace".to_owned())),
        KeyCode::Tab => named("Tab"),
        KeyCode::BackTab => Some(LiveInput::Key("BTab".to_owned())),
        KeyCode::Up => named("Up"),
        KeyCode::Down => named("Down"),
        KeyCode::Left => named("Left"),
        KeyCode::Right => named("Right"),
        KeyCode::Home => named("Home"),
        KeyCode::End => named("End"),
        KeyCode::PageUp => named("PPage"),
        KeyCode::PageDown => named("NPage"),
        KeyCode::Delete => named("DC"),
        KeyCode::Insert => named("IC"),
        KeyCode::F(n) => named(&format!("F{n}")),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    fn named(name: &str) -> Option<LiveInput> {
        Some(LiveInput::Key(name.to_owned()))
    }

    #[test]
    fn keys_reach_the_agent_by_their_tmux_names() {
        let none = KeyModifiers::NONE;
        let ctrl = KeyModifiers::CONTROL;
        assert_eq!(
            input_for(&key(KeyCode::Char('字'), none)),
            Some(LiveInput::Text("字".to_owned())),
            "a character is typed as itself, whatever script it is in"
        );
        assert_eq!(
            input_for(&key(KeyCode::Char('Q'), KeyModifiers::SHIFT)),
            Some(LiveInput::Text("Q".to_owned()))
        );
        assert_eq!(input_for(&key(KeyCode::Char('c'), ctrl)), named("C-c"));
        assert_eq!(input_for(&key(KeyCode::Char(' '), ctrl)), named("C-Space"));
        assert_eq!(
            input_for(&key(KeyCode::Char('f'), KeyModifiers::ALT)),
            named("M-f")
        );
        assert_eq!(input_for(&key(KeyCode::Enter, none)), named("Enter"));
        assert_eq!(
            input_for(&key(KeyCode::Enter, KeyModifiers::SHIFT)),
            named("M-Enter")
        );
        assert_eq!(input_for(&key(KeyCode::Esc, none)), named("Escape"));
        assert_eq!(input_for(&key(KeyCode::Backspace, none)), named("BSpace"));
        assert_eq!(
            input_for(&key(KeyCode::BackTab, KeyModifiers::SHIFT)),
            named("BTab")
        );
        assert_eq!(
            input_for(&key(KeyCode::Up, KeyModifiers::SHIFT)),
            named("S-Up")
        );
        assert_eq!(input_for(&key(KeyCode::Left, ctrl)), named("C-Left"));
        assert_eq!(input_for(&key(KeyCode::PageUp, none)), named("PPage"));
        assert_eq!(input_for(&key(KeyCode::F(5), none)), named("F5"));
    }

    #[test]
    fn ctrl_bracket_is_the_way_out() {
        assert!(is_leave(&key(KeyCode::Char(']'), KeyModifiers::CONTROL)));
        assert!(
            is_leave(&key(KeyCode::Char('5'), KeyModifiers::CONTROL)),
            "as a terminal without the enhanced keyboard reports it"
        );
        assert!(!is_leave(&key(KeyCode::Char(']'), KeyModifiers::NONE)));
        assert!(
            !is_leave(&key(KeyCode::Char('b'), KeyModifiers::CONTROL)),
            "ctrl+b is tmux's prefix, and the agent's"
        );
        assert!(!is_leave(&key(KeyCode::Esc, KeyModifiers::NONE)));
    }
}
