//! Reads what a Codex-backed Dispatch has spent from Codex's own session files.
//!
//! **Verified, not assumed**: Codex writes one `rollout-*.jsonl` per thread under
//! `~/.codex/sessions/YYYY/MM/DD/`. Its first line is a `session_meta` naming the thread's
//! working directory and the session it belongs to; `turn_context` lines name the model each
//! turn runs on; `token_count` events carry each model call's usage. Nothing ties a rollout
//! to a worktree but that working directory, so a worktree's session is the newest top-level
//! thread started in it, together with the threads it spawned (which share its session id).
//! Unlike Claude Code, a Codex rollout counts cached and cache-written tokens inside
//! `input_tokens`, and reasoning inside `output_tokens`.

use crate::environment::Environment;
use crate::pricing::{Tokens, Usage};
use crate::transcript::Session;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub fn codex_sessions_dir(env: &dyn Environment) -> Option<PathBuf> {
    env.home_dir().map(|home| home.join(".codex").join("sessions"))
}

/// What a rollout's first line says about the thread it records.
#[derive(Clone, Debug, PartialEq, Eq)]
struct Meta {
    cwd: String,
    session_id: String,
    /// The thread was started by a person or `codex exec`, not spawned by another thread.
    top_level: bool,
}

fn read_meta(path: &Path) -> Option<Meta> {
    let mut first = String::new();
    BufReader::new(fs::File::open(path).ok()?)
        .read_line(&mut first)
        .ok()?;
    let entry: Value = serde_json::from_str(&first).ok()?;
    if entry.get("type").and_then(Value::as_str) != Some("session_meta") {
        return None;
    }
    let payload = entry.get("payload")?;
    let id = payload.get("id").and_then(Value::as_str)?;
    Some(Meta {
        cwd: payload.get("cwd").and_then(Value::as_str)?.to_string(),
        session_id: payload
            .get("session_id")
            .and_then(Value::as_str)
            .unwrap_or(id)
            .to_string(),
        top_level: payload.get("source").is_some_and(Value::is_string),
    })
}

fn rollouts(dir: &Path, found: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        if path.is_dir() {
            rollouts(&path, found);
        } else if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            found.push(path);
        }
    }
}

/// Remembers each rollout's first line, which never changes once written, so finding a
/// worktree's session on every poll costs a directory walk rather than a read of every file.
#[derive(Debug, Default)]
pub struct Locator {
    metas: HashMap<PathBuf, Option<Meta>>,
}

impl Locator {
    /// Every rollout of the newest Codex session started in `worktree`: its top-level thread
    /// first, then the threads that thread spawned.
    pub fn find(&mut self, sessions_root: &Path, worktree: &str) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        rollouts(sessions_root, &mut paths);
        let mut matching = Vec::new();
        for path in paths {
            let meta = self
                .metas
                .entry(path.clone())
                .or_insert_with(|| read_meta(&path));
            // A rollout still being created has no first line yet; read it again next time.
            let Some(meta) = meta.clone() else {
                self.metas.remove(&path);
                continue;
            };
            if meta.cwd == worktree {
                matching.push((path, meta));
            }
        }
        let modified = |path: &Path| {
            fs::metadata(path)
                .and_then(|m| m.modified())
                .unwrap_or(SystemTime::UNIX_EPOCH)
        };
        let Some((main, main_meta)) = matching
            .iter()
            .filter(|(_, meta)| meta.top_level)
            .max_by_key(|(path, _)| modified(path))
            .cloned()
        else {
            return Vec::new();
        };
        let mut session = vec![main.clone()];
        session.extend(
            matching
                .into_iter()
                .filter(|(path, meta)| *path != main && meta.session_id == main_meta.session_id)
                .map(|(path, _)| path),
        );
        session
    }
}

/// Reads spend, model, effort and context from a session's rollouts. The model, effort and
/// context are its top-level thread's; spend is every thread's.
pub fn session(paths: &[PathBuf]) -> Session {
    let mut session = Session::default();
    for (index, path) in paths.iter().enumerate() {
        let Ok(contents) = fs::read_to_string(path) else {
            continue;
        };
        let thread = read_thread(&contents);
        session.usage.merge(&thread.usage);
        if index == 0 {
            session.model = thread.model;
            session.effort = thread.effort;
            session.context = thread.context;
        }
    }
    session
}

fn read_thread(contents: &str) -> Session {
    let mut session = Session::default();
    let mut usage = Usage::default();
    let mut model: Option<String> = None;
    // The same call's usage is reported again whenever only the rate limits changed.
    let mut last_total: Option<Value> = None;
    for line in contents.lines() {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let Some(payload) = entry.get("payload") else {
            continue;
        };
        match entry.get("type").and_then(Value::as_str) {
            Some("turn_context") => {
                model = payload.get("model").and_then(Value::as_str).map(str::to_string);
                if let Some(effort) = payload
                    .get("effort")
                    .and_then(Value::as_str)
                    .or_else(|| {
                        payload
                            .pointer("/collaboration_mode/settings/reasoning_effort")
                            .and_then(Value::as_str)
                    })
                {
                    session.effort = Some(effort.to_string());
                }
            }
            Some("event_msg")
                if payload.get("type").and_then(Value::as_str) == Some("token_count") =>
            {
                let Some(info) = payload.get("info").filter(|info| !info.is_null()) else {
                    continue;
                };
                let total = info.get("total_token_usage").cloned();
                if total.is_some() && total == last_total {
                    continue;
                }
                last_total = total;
                let (Some(model), Some(last)) = (model.as_deref(), info.get("last_token_usage"))
                else {
                    continue;
                };
                let count = |key: &str| last.get(key).and_then(Value::as_u64).unwrap_or(0);
                let prompt = count("input_tokens");
                let cached = count("cached_input_tokens");
                let written = count("cache_write_input_tokens");
                usage.add(
                    model,
                    &Tokens {
                        input: prompt.saturating_sub(cached + written),
                        output: count("output_tokens"),
                        cache_write_5m: written,
                        cache_write_1h: 0,
                        cache_read: cached,
                    },
                );
                session.context = Some(prompt);
            }
            _ => {}
        }
    }
    session.usage = usage;
    session.model = model;
    session
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn meta(id: &str, session_id: &str, cwd: &str, source: Value) -> String {
        json!({"type": "session_meta", "payload": {
            "id": id, "session_id": session_id, "cwd": cwd, "source": source}})
        .to_string()
    }

    fn turn(model: &str) -> String {
        json!({"type": "turn_context", "payload": {"model": model, "effort": "high"}}).to_string()
    }

    fn tokens(total: u64, input: u64, cached: u64, output: u64) -> String {
        let usage = |n: u64| json!({"input_tokens": n, "cached_input_tokens": cached,
                                     "cache_write_input_tokens": 0, "output_tokens": output});
        json!({"type": "event_msg", "payload": {"type": "token_count", "info": {
            "total_token_usage": usage(total), "last_token_usage": usage(input)}}})
        .to_string()
    }

    #[test]
    fn cached_tokens_are_taken_out_of_input_and_a_repeated_count_is_not_spent_twice() {
        let contents = [
            meta("a", "a", "/w", json!("cli")),
            turn("gpt-6-sol"),
            json!({"type": "event_msg", "payload": {"type": "token_count", "info": null}})
                .to_string(),
            tokens(1_000, 1_000, 400, 50),
            tokens(1_000, 1_000, 400, 50),
            tokens(2_500, 1_500, 400, 50),
        ]
        .join("\n");
        let session = read_thread(&contents);
        let spent = session.usage.tokens();
        assert_eq!(spent.input, 600 + 1_100);
        assert_eq!(spent.cache_read, 800);
        assert_eq!(spent.output, 100);
        assert_eq!(session.model.as_deref(), Some("gpt-6-sol"));
        assert_eq!(session.effort.as_deref(), Some("high"));
        assert_eq!(session.context, Some(1_500));
        assert!(session.usage.cost().is_some());
    }

    #[test]
    fn a_worktree_session_is_its_newest_top_level_thread_and_the_threads_it_spawned() {
        let root = tempfile::tempdir().unwrap();
        let day = root.path().join("2026/09/27");
        fs::create_dir_all(&day).unwrap();
        let write = |name: &str, first: String| {
            let path = day.join(name);
            fs::write(&path, format!("{first}\n{}\n{}", turn("gpt-5.6-terra"), tokens(10, 10, 0, 1)))
                .unwrap();
            path
        };
        let spawned = json!({"subagent": {"thread_spawn": {"parent_thread_id": "main"}}});
        let main = write("rollout-2.jsonl", meta("main", "main", "/w", json!("cli")));
        let child = write("rollout-3.jsonl", meta("child", "main", "/w", spawned));
        write("rollout-4.jsonl", meta("other", "other", "/elsewhere", json!("cli")));
        let old = write("rollout-1.jsonl", meta("old", "old", "/w", json!("exec")));
        let earlier = SystemTime::now() - std::time::Duration::from_secs(3_600);
        fs::File::options().write(true).open(&old).unwrap().set_modified(earlier).unwrap();

        let found = Locator::default().find(root.path(), "/w");
        assert_eq!(found, vec![main, child]);
        assert_eq!(session(&found).usage.tokens().input, 20);
        assert!(Locator::default().find(root.path(), "/nowhere").is_empty());
    }
}
