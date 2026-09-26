use crate::environment::Environment;
use crate::error::{codes, err};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Checklist {
    pub items: Vec<String>,
    pub checked: Vec<bool>,
}

pub struct ChecklistStore {
    dir: Option<PathBuf>,
}

impl ChecklistStore {
    pub fn open(env: &dyn Environment) -> Self {
        Self {
            dir: crate::environment::checklist_dir(env),
        }
    }

    pub fn for_dir(dir: Option<PathBuf>) -> Self {
        Self { dir }
    }

    fn path(&self, run_id: &str) -> Result<PathBuf> {
        let dir = self
            .dir
            .clone()
            .ok_or_else(|| err(codes::INTERNAL_ERROR, "no checklist directory available"))?;
        Ok(dir.join(format!("{run_id}.json")))
    }

    pub fn load(&self, run_id: &str) -> Result<Checklist> {
        let path = self.path(run_id)?;
        let content = match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(_) => return Ok(Checklist::default()),
        };
        Ok(serde_json::from_str(&content)?)
    }

    pub fn save(&self, run_id: &str, checklist: &Checklist) -> Result<()> {
        let path = self.path(run_id)?;
        fs::create_dir_all(path.parent().unwrap())?;
        fs::write(path, serde_json::to_string_pretty(checklist)?)?;
        Ok(())
    }

    /// Replaces the whole checklist and clears checked state.
    pub fn replace_items(&self, run_id: &str, items: Vec<String>) -> Result<Checklist> {
        let checked = vec![false; items.len()];
        let checklist = Checklist { items, checked };
        self.save(run_id, &checklist)?;
        Ok(checklist)
    }

    /// `n` is 1-based, per the CLI contract.
    pub fn set_checked(&self, run_id: &str, n: usize, value: bool) -> Result<Checklist> {
        let mut checklist = self.load(run_id)?;
        if checklist.items.is_empty() {
            return Err(err(codes::INVALID_INPUT, "no checklist is set for this run"));
        }
        let index = n
            .checked_sub(1)
            .filter(|i| *i < checklist.items.len())
            .ok_or_else(|| err(codes::INVALID_INPUT, format!("no checklist item {n}")))?;
        checklist.checked[index] = value;
        self.save(run_id, &checklist)?;
        Ok(checklist)
    }
}
