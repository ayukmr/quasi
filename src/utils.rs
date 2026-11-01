use std::path::PathBuf;

use anyhow::{Context, Result};

pub fn config_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("$HOME not set")?;
    Ok(PathBuf::from(home).join(".config/quasi"))
}

pub fn convo_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME").context("$HOME not set")?;
    Ok(PathBuf::from(home).join(".config/quasi/convo"))
}
