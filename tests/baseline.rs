// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

use oat_agents::launch::prompt::{
    LAUNCH_PROTOCOL_CLAUDE, LAUNCH_PROTOCOL_CODEX, OAT_CONSOLE_BASELINE, OAT_META_BASELINE, OAT_SYSTEM_VIEW_SKILL,
    PREAMBLE_CONSOLE, PREAMBLE_CORE_ROLE, PREAMBLE_ROLE, ROLE_PROTOCOL,
};
use oat_agents::store::MessageKind;

/// Plugin role names ADR-0001 assigns to the example plugin, never the core's — a hit here
/// means a plugin's practice leaked into the core's own text.
const ROLE_DENYLIST: &[&str] = &["planner", "worker", "reviewer", "tester"];

/// Team-policy phrases ADR-0004 says belong to a plugin or a goal, never `meta fire` or
/// the baseline it ships.
const POLICY_DENYLIST: &[&str] = &[
    "review round",
    "max-review-rounds",
    "retry limit",
    "epic pull request",
];

#[test]
fn the_baseline_names_no_role_the_core_does_not_own_and_states_no_team_policy() {
    for text in [OAT_META_BASELINE, ROLE_PROTOCOL] {
        let lower = text.to_lowercase();
        for word in ROLE_DENYLIST {
            assert!(
                !lower.contains(word),
                "the core's baseline text must not name plugin role '{word}': {text}"
            );
        }
        for phrase in POLICY_DENYLIST {
            assert!(
                !lower.contains(phrase),
                "the core's baseline text must not state team policy '{phrase}': {text}"
            );
        }
    }
}

/// Words `.dev_docs/CONTEXT.md` retired. An agent reading one of them in its own prompt is
/// taught a name the CLI and the rest of the prompt no longer use.
const RETIRED_WORDS: &[&str] = &["coordinator", "big plan", "big-plan", "worker_done", "objective"];

#[test]
fn the_core_prompts_use_the_glossarys_words() {
    for text in [
        PREAMBLE_CORE_ROLE,
        PREAMBLE_ROLE,
        PREAMBLE_CONSOLE,
        OAT_META_BASELINE,
        OAT_CONSOLE_BASELINE,
        ROLE_PROTOCOL,
        LAUNCH_PROTOCOL_CLAUDE,
        LAUNCH_PROTOCOL_CODEX,
        OAT_SYSTEM_VIEW_SKILL,
    ] {
        let lower = text.to_lowercase();
        for word in RETIRED_WORDS {
            assert!(!lower.contains(word), "a core prompt still says '{word}': {text}");
        }
    }
    assert!(PREAMBLE_CORE_ROLE.contains("goal"), "the meta-agent is told where its goal is");
}

#[test]
fn the_meta_agent_baseline_names_every_inbox_message_kind_the_cli_sends() {
    for kind in [
        MessageKind::MemberDone,
        MessageKind::Question,
        MessageKind::Escalation,
        MessageKind::LaunchFailed,
    ] {
        let name = serde_json::to_value(&kind).unwrap();
        let name = name.as_str().unwrap();
        assert!(
            OAT_META_BASELINE.contains(&format!("`{name}`")),
            "the meta-agent baseline does not name the inbox kind `{name}`"
        );
    }
}
