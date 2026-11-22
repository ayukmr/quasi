use crate::sse::Content;

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

pub fn set_echo(on: bool) {
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(libc::STDIN_FILENO, &mut t) != 0 {
            return;
        }

        if on {
            t.c_lflag |= libc::ECHO;
        } else {
            t.c_lflag &= !libc::ECHO;
        }

        libc::tcsetattr(libc::STDIN_FILENO, libc::TCSANOW, &t);
    }
}

pub fn flush_input() {
    unsafe { libc::tcflush(libc::STDIN_FILENO, libc::TCIFLUSH); }
}

pub fn text(s: String) -> Vec<Content> {
    vec![Content::Text { text: s, cache_control: None }]
}
