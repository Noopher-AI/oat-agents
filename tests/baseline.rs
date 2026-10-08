// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

use oat_agents::launch::prompt::{OAT_META_BASELINE, ROLE_PROTOCOL};

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
