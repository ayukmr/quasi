use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use anyhow::Result;
use quasi::agent::Agent;
use quasi::render::Renderer;
use rustyline::DefaultEditor;

fn main() -> Result<()> {
    let rnd1 = Arc::new(Mutex::new(Renderer::new()));
    let rnd2 = rnd1.clone();
    let rnd3 = rnd1.clone();

    thread::spawn(move || {
        loop {
            rnd2.lock().unwrap().tick();
            thread::sleep(Duration::from_millis(50));
        }
    });

    let mut rl = DefaultEditor::new()?;
    let mut agent = Agent::new(rnd3)?;

    loop {
        rnd1.lock().unwrap().divider();

        let start = rnd1.lock().unwrap().t;
        while rnd1.lock().unwrap().t == start {
            thread::sleep(Duration::from_millis(10));
        }

        let readline = rl.readline("\x1b[35m> \x1b[0m");
        match readline {
            Ok(prompt) => agent.prompt(&prompt)?,
            _ => break,
        }
    }

    Ok(())
}
