use std::{env, path::Path};
use std::path::PathBuf;

fn home() -> PathBuf {
    PathBuf::from(env::var_os("HOME").unwrap())
}

pub fn config_dir() -> PathBuf {
    let home = home();
    home.join(".config/quasi")
}

pub fn state_dir() -> PathBuf {
    let home = home();
    home.join(".local/state/quasi")
}

pub fn tools_dir() -> PathBuf {
    config_dir().join("tools")
}

pub fn expand_tilde_path(p: &Path) -> PathBuf {
    let home = home();

    if let Ok(rest) = p.strip_prefix("~") {
        home.join(rest)
    } else {
        PathBuf::from(p)
    }
}

pub fn expand_tilde(p: &str) -> PathBuf {
    expand_tilde_path(&PathBuf::from(p))
}

pub fn compact_tilde(p: &Path) -> String {
    let home = home();

    if let Ok(rest) = p.strip_prefix(home) {
        PathBuf::from("~").join(rest)
    } else {
        p.to_path_buf()
    }.to_string_lossy().to_string()
}
