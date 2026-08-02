use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

const STATE_FILE: &str = "/var/lib/caustic-ota/state.json";

#[derive(Serialize, Deserialize)]
pub struct State {
    pub current_version: String,
}

impl State {
    pub fn read_or_default() -> Self {
        Self::read().unwrap_or(Self {
            current_version: String::new(),
        })
    }

    pub fn read() -> Result<Self> {
        let bytes = fs::read(STATE_FILE).context("read state")?;
        serde_json::from_slice(&bytes).context("parse state")
    }

    pub fn write(&self) -> Result<()> {
        let path = Path::new(STATE_FILE);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
        }
        let bytes = serde_json::to_vec_pretty(self)?;
        fs::write(path, bytes).context("write state")
    }
}
