use quasi::{Agent, Renderer};

use std::{env, thread};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::Result;
use rustyline::DefaultEditor;

const TICK: Duration = Duration::from_millis(30);
const MODELS: &[&str] = &["fable-5-1", "opus-5-5", "sonnet-5-5", "haiku-5-5"];

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

        let readline = rl.readline(&format!("\x1b[35m{}>\x1b[0m ", agent.model));
        match readline {
            Ok(prompt) => {
                if prompt.is_empty() {
                    continue;
                }

                if let Some(name) = prompt.strip_prefix("/save ") {
                    agent.save(name)?;
                    renderer.lock().unwrap().write_success(&format!("saved to {name}"));
                } else if let Some(model) = prompt.strip_prefix("/model ") {
                    if MODELS.contains(&model) {
                        agent.model = model.to_string();
                        renderer.lock().unwrap().write_success(&format!("changed to {model}"));
                    } else {
                        renderer.lock().unwrap().write_error(&format!("invalid model {model}"));
                    }
                } else if prompt.starts_with("/") {
                    renderer.lock().unwrap().write_error(&format!("invalid command {prompt}"));
                } else {
                    agent.prompt(&prompt)?;
                }
            },
            _ => break,
        }
    }

    Ok(())
}
