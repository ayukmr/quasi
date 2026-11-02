use std::io::{self, Write};

pub struct Renderer {
    t: usize,
    status: Option<String>,
    fresh: bool,
    last: String,
    queue: String,
}

fn cols() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

const BG: &str = "\x1b[40m";

pub fn with_bg(s: &str) -> String {
    s.lines()
        .map(|l| {
            format!(
                "{BG}{}\x1b[K\x1b[0m",
                l.replace("\x1b[0m", &format!("\x1b[0m{BG}")),
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn rows(s: &str) -> usize {
    let cols = cols();
    s.chars().count().max(1).div_ceil(cols)
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            t: 0,
            status: None,
            fresh: false,
            last: "".to_string(),
            queue: "".to_string(),
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

    pub fn write_pretty(&mut self, delta: &str, full: &str) {
        let up = rows(&self.last);
        self.last += delta;

        let nls = self.last.matches('\n').count();
        let start = full.rmatch_indices('\n').nth(nls).map(|(i, _)| i + 1).unwrap_or(0);
        let tail = with_bg(&full[start..]);

        if self.status.is_some() && !self.fresh {
            self.queue += &format!("\x1b[{up}A");
        }
        self.fresh = false;

        self.queue += &format!("\r\x1b[J{tail}\n");

        if let Some((_, rest)) = self.last.rsplit_once('\n') {
            self.last = rest.to_string();
        }
    }

    pub fn write_line(&mut self, s: &str) {
        self.queue += &format!("{s}\n");
        self.last.clear();
    }

    pub fn write_success(&mut self, s: &str) {
        self.write_line(&format!("{s} \x1b[32m✓\x1b[0m"));
    }

    pub fn write_error(&mut self, s: &str) {
        self.write_line(&format!("{s} \x1b[31m✗\x1b[0m"));
    }

    const SPIN_FRAMES: &[&str] = &["⠎⠀", "⠊⠁", "⠈⠑", "⠀⠱", "⠀⡰", "⢀⡠", "⢄⡀", "⢆⠀"];

    pub fn flush(&mut self) {
        print!("{}", self.queue);
        self.queue.clear();
        io::stdout().flush().ok();
    }

    pub fn tick(&mut self) {
        self.flush();

        let Some(status) = &self.status else {
            return
        };

        self.t = (self.t + 1) % (Self::SPIN_FRAMES.len() * 2);

        print!("\r\x1b[2K\x1b[35m{}\x1b[0m {status}...", Self::SPIN_FRAMES[self.t / 2]);
        io::stdout().flush().ok();
    }

    pub fn confirm(&mut self) -> bool {
        self.flush();

        loop {
            print!("\x1b[31mallow?> \x1b[0m");
            io::stdout().flush().ok();

            let mut ans = String::new();
            io::stdin().read_line(&mut ans).unwrap();
            ans = ans.trim().to_lowercase();

            if ans == "y" || ans == "yes" {
                return true;
            } else if ans == "n" || ans == "no" {
                return false;
            }
        }
    }

    pub fn finish(&mut self) {
        if !self.last.is_empty() {
            self.queue += "\n";
            self.last.clear();
        }
    }
}
