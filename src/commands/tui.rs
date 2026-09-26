use crate::environment::Environment;
use anyhow::Result;
use serde_json::{json, Value};

/// The live view. Everything it decides — what to render, what a key does — lives in `tui`,
/// tested on its own against recorded Runs; this only hands it the environment.
pub fn run(env: &dyn Environment) -> Result<Value> {
    crate::tui::run(env)?;
    Ok(json!({"exited": true}))
}
