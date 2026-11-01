use std::io::{self, Write};

pub struct Renderer {
    pub t: usize,
    status: Option<String>,
    was_active: bool,
    last: String,
    queue: String,
}

fn cols() -> usize {
    terminal_size::terminal_size()
        .map(|(w, _)| w.0 as usize)
        .unwrap_or(80)
}

fn rows(s: &str) -> usize {
    let cols = cols();
    s.len().max(1).div_ceil(cols)
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            t: 0,
            status: None,
            was_active: false,
            last: "".to_string(),
            queue: "".to_string(),
        }
    }

    pub fn set_status(&mut self, status: Option<String>) {
        match (&status, self.was_active) {
            (Some(_), false) => {
                self.queue += "\n";
                self.was_active = true;
            }
            (None, true) => {
                self.queue += "\r\x1b[J\x1b[1A\r";
                self.was_active = false;
            }
            _ => {}
        }

        self.status = status;
    }

    // pub fn write(&mut self, s: &str) {
    //     if self.was_active {
    //         let up = rows(&self.last);
    //         self.last += s;
    //         self.queue += &format!("\x1b[{up}A\r\x1b[J{}\n", self.last);
    //     } else {
    //         self.last += s;
    //         self.queue += s;
    //     }

    //     if let Some((_, rest)) = self.last.rsplit_once('\n') {
    //         self.last = rest.to_string();
    //     }
    // }

    pub fn write_pretty(&mut self, s: &str, p: &str) {
        let up = rows(&self.last);
        self.last += s;

        let nls = self.last.matches('\n').count();
        let start = p.rmatch_indices('\n').nth(nls).map(|(i, _)| i + 1).unwrap_or(0);
        let tail = &p[start..];

        if self.was_active {
            self.queue += &format!("\x1b[{up}A");
        }

        self.queue += &format!("\r\x1b[J{tail}\n");

        if let Some((_, rest)) = self.last.rsplit_once('\n') {
            self.last = rest.to_string();
        }
    }

    pub fn write_line(&mut self, p: &str) {
        self.queue += &format!("{p}\n");
        self.last.clear();
    }

    const SPIN_FRAMES: &[&str] = &["⠎⠀", "⠊⠁", "⠈⠑", "⠀⠱", "⠀⡰", "⢀⡠", "⢄⡀", "⢆⠀"];

    pub fn tick(&mut self) {
        print!("{}", self.queue);
        self.queue.clear();
        io::stdout().flush().ok();

        self.t = (self.t + 1) % Self::SPIN_FRAMES.len();

        let Some(status) = &self.status else {
            return
        };

        print!("\r\x1b[2K\x1b[35m{}\x1b[0m {status}...", Self::SPIN_FRAMES[self.t]);
        io::stdout().flush().ok();
    }

    pub fn confirm(&mut self) -> bool {
        loop {
            self.queue += "\x1b[31mallow?> \x1b[0m";
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

    pub fn divider(&mut self) {
        self.queue += &format!("{}\n", "—".repeat(cols()));
    }
}
