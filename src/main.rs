use quasi::{Agent, Renderer};

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use rustyline::DefaultEditor;

const TICK: Duration = Duration::from_millis(30);

fn main() -> Result<()> {
    let renderer = Arc::new(Mutex::new(Renderer::new()));
    let renderer_cln = renderer.clone();

    thread::spawn(move || {
        loop {
            renderer_cln.lock().unwrap().tick();
            thread::sleep(TICK);
        }
    });

    let mut rl = DefaultEditor::new()?;
    let mut agent = Agent::new(renderer.clone())?;

    loop {
        renderer.lock().unwrap().flush();

        let readline = rl.readline("\x1b[35m> \x1b[0m");
        match readline {
            Ok(prompt) => agent.prompt(&prompt)?,
            _ => break,
        }

        renderer.lock().unwrap().write_line("");
    }

    Ok(())
}
