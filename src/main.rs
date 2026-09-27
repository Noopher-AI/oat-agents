// SPDX-License-Identifier: MIT
// Copyright (c) 2026 oat-agents contributors

use clap::Parser;
use oat_agents::environment::SystemEnvironment;
use oat_agents::error::{codes, to_failure};
use oat_agents::role::memory::InMemoryCatalogBuilder;
use oat_agents::Cli;

fn main() {
    let cli = Cli::parse();
    let env = SystemEnvironment;
    // F1 ships no plugin loader (F4/F5); the real binary has no source of role or core-role
    // instructions yet, so it runs against an empty catalog until then.
    let catalog = InMemoryCatalogBuilder::new().build();

    match oat_agents::execute(cli, &env, &catalog) {
        Ok(value) => {
            println!("{}", serde_json::to_string_pretty(&value).unwrap());
        }
        Err(error) => {
            let failure = to_failure(&error);
            eprintln!("{}", serde_json::to_string_pretty(&failure.to_json()).unwrap());
            std::process::exit(exit_code_for(&failure.code));
        }
    }
}

fn exit_code_for(code: &str) -> i32 {
    if code == codes::INVALID_CLI_ARGUMENTS || code == codes::CLAP_DISPLAY {
        2
    } else {
        1
    }
}
