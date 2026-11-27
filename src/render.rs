use crate::utils;
use crate::tools::Tool;

use std::collections::HashMap;
use std::io::{self, Write};
use std::os::unix::process::CommandExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};

use rustyline::DefaultEditor;
use rustyline::error::ReadlineError;
use serde_json::Value;

pub static HAS_GLOW: AtomicBool = AtomicBool::new(false);
pub static HAS_BAT: AtomicBool = AtomicBool::new(false);

pub fn glow(text: &str) -> String {
    if !HAS_GLOW.load(Ordering::SeqCst) {
        return text.to_string();
    }

    let (reader, mut writer) = io::pipe().unwrap();

    writer.write_all(text.as_bytes()).unwrap();
    drop(writer);

    let output = Command::new("glow")
        .process_group(0)
        .args(["-s", "auto", "-w", &cols().to_string(), "-"])
        .stdin(reader)
        .output()
        .unwrap();

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|l| l.strip_prefix("  ").unwrap_or(l)).collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

pub fn bat(text: &str, extra: &[&str]) -> String {
    if !HAS_BAT.load(Ordering::SeqCst) {
        return text.to_string();
    }

    let (reader, mut writer) = io::pipe().unwrap();

    writer.write_all(text.as_bytes()).unwrap();
    drop(writer);

    let output = Command::new("bat")
        .process_group(0)
        .args(["-p", "--theme", "base16", "--color", "always", "-"])
        .args(extra)
        .stdin(reader)
        .output()
        .unwrap();

    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

pub fn tokens(n: u32) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f32 / 1_000_000.0)
    } else if n >= 10_000 {
        format!("{}k", n / 1000)
    } else if n >= 100 {
        format!("{:.1}k", n as f32 / 1000.0)
    } else {
        n.to_string()
    }
}

pub fn dimmed(s: &str) -> String {
    s.split('\n')
        .map(|l| format!("\x1b[2m{l}\x1b[0m"))
        .collect::<Vec<_>>()
        .join("\n")
}

fn cols() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

fn rows(s: &str) -> usize {
    let cols = cols();
    s.split("\n").map(|l| ansi_width::ansi_width(l).max(1).div_ceil(cols)).sum()
}

pub fn tool(name: &str, tool: Option<&Tool>, input: &HashMap<String, Value>) -> String {
    let disp = tool.and_then(|t| (t.render)(input.clone()).ok());

    disp.unwrap_or_else(|| {
        let mut args: Vec<(&String, &Value)> = input.iter().collect();
        args.sort_unstable_by_key(|(k, _)| *k);

        format!(
            "\x1b[1m{}\x1b[0m({})",
            name,
            args.iter().map(|(k, v)| format!("{k}: \x1b[36m{v}\x1b[0m")).collect::<Vec<_>>().join(", "),
        )
    })
}

pub struct Renderer {
    t: usize,
    status: Option<String>,
    fresh: bool,
    last: String,
    old: String,
    queue: String,
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            t: 0,
            status: None,
            fresh: false,
            last: "".to_string(),
            queue: "".to_string(),
            old: "".to_string(),
        }
    }

    pub fn set_status(&mut self, status: Option<&str>) {
        match (status, self.status.is_some()) {
            (Some(_), false) => {
                self.fresh = true;
            }
            (None, true) => {
                self.queue += if self.fresh { "\r\x1b[J" } else { "\r\x1b[J\x1b[1A\r" };
            }
            _ => {}
        }

        self.status = status.map(|s| s.to_string());
    }

    pub fn write_plain(&mut self, delta: &str) {
        let up = rows(&self.last);
        self.last += delta;

        if self.status.is_some() && !self.fresh {
            self.queue += &format!("\x1b[{up}A");
        }
        self.fresh = false;

        self.queue += &format!("\r\x1b[J{}\n", dimmed(&self.last));

        if let Some((_, rest)) = self.last.rsplit_once('\n') {
            self.last = rest.to_string();
        }
    }

    pub fn write_pretty(&mut self, full: &str) {
        let start: usize =
            self.old.chars()
                .zip(full.chars())
                .take_while(|(o, n)| o == n)
                .map(|(c, _)| c.len_utf8())
                .sum();

        let line_start = full[..start].rfind("\n").map_or(0, |n| n + 1);
        let tail = &full[line_start..];

        let up = rows(&self.old[line_start..]);

        if !self.fresh {
            self.queue += &format!("\x1b[{up}A");
        }
        self.queue += "\r\x1b[J";
        self.fresh = false;

        self.queue += &format!("{tail}\n");
        self.old = full.to_string();
    }

    pub fn write_line(&mut self, s: &str) {
        self.queue += &format!("\r\x1b[2K{s}\n");
        self.last.clear();
    }

    pub fn write_success(&mut self, s: &str) {
        self.write_line(&format!("{s} \x1b[32m✓\x1b[0m"));
    }

    pub fn write_error(&mut self, s: &str) {
        self.write_line(&format!("{s} \x1b[31m✗\x1b[0m"));
    }

    const SPIN_FRAMES: &[&str] = &[
        "⠀⠑", "⠀⠰", "⠀⡰", "⢀⡠", "⢄⡀", "⢆⠀", "⢎⠀", "⠎⠁",
        "⠊⠑", "⠈⠱", "⠀⡱", "⢀⡰", "⢄⡠", "⢆⡀", "⠎⠀", "⠊⠁",
        "⠈⠑", "⠀⠱", "⠀⡠", "⢀⡀", "⢄⠀", "⠆⠀", "⠊⠀", "⠈⠁",
    ];

    pub fn flush(&mut self) {
        print!("\x1b[?2026h{}\x1b[?2026l", self.queue);

        self.queue.clear();
        io::stdout().flush().ok();
    }

    pub fn tick(&mut self) {
        let Some(status) = &self.status else {
            self.flush();
            return;
        };

        self.t = (self.t + 1) % (Self::SPIN_FRAMES.len() * 2);
        self.queue += &format!("\r\x1b[2K\x1b[35m{}\x1b[0m {status}...", Self::SPIN_FRAMES[self.t / 2]);

        self.flush();
    }

    pub fn confirm(&mut self) -> bool {
        self.flush();

        utils::flush_input();
        utils::set_echo(true);
        let mut rl = DefaultEditor::new().unwrap();

        loop {
            match rl.readline("\x1b[31mallow? (y/n)> \x1b[0m") {
                Ok(s) => {
                    let ans = s.trim().to_lowercase();
                    if ans == "y" || ans == "yes" {
                        utils::set_echo(false);
                        return true;
                    } else if ans == "n" || ans == "no" {
                        utils::set_echo(false);
                        return false;
                    }
                }
                Err(ReadlineError::Interrupted | ReadlineError::Eof) => return false,
                _ => {}
            }
        }
    }

    pub fn finish(&mut self) {
        self.queue += "\n";
        self.last.clear();
    }
}
