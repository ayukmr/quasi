use quasi::{Agent, Renderer};

use std::{env, thread};
use std::sync::{Arc, Mutex};
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

    let mut agent = if let Some(name) = env::args().nth(1) {
        Agent::load(renderer.clone(), &name)
    } else {
        Agent::new(renderer.clone())
    }?;

    loop {
        renderer.lock().unwrap().write_line("");
        renderer.lock().unwrap().flush();

        let readline = rl.readline("\x1b[35m>\x1b[0m ");
        match readline {
            Ok(prompt) => {
                if prompt.is_empty() {
                    continue;
                }

                if let Some(name) = prompt.strip_prefix("/save ") {
                    agent.save(name)?;
                    renderer.lock().unwrap().write_success(&format!("saved to {name}"));
                } else {
                    agent.prompt(&prompt)?;
                }
            },
            _ => break,
        }
    }

    Ok(())
}
