use quasi::{Agent, Renderer, agent, prompt, render, utils};

use std::{env, thread};
use std::sync::{Arc, Mutex};
use std::sync::atomic::Ordering;
use std::time::Duration;

use anyhow::Result;
use rustyline::error::ReadlineError;

const TICK: Duration = Duration::from_millis(30);

fn main() -> Result<()> {
    let renderer = Arc::new(Mutex::new(Renderer::new()));
    let renderer2 = renderer.clone();

    ctrlc::set_handler(move || agent::INTERRUPTED.store(true, Ordering::SeqCst))?;

    thread::spawn(move || {
        loop {
            renderer2.lock().unwrap().tick();
            thread::sleep(TICK);
        }
    });

    let mut rl = prompt::editor()?;

    let mut agent = if let Some(name) = env::args().nth(1) {
        Agent::load(renderer.clone(), &name)?
    } else {
        Agent::new(renderer.clone())?
    };

    loop {
        renderer.lock().unwrap().write_line("");
        renderer.lock().unwrap().flush();

        utils::flush_input();
        utils::set_echo(true);

        let mut icons = Vec::new();
        if agent::INTERRUPTED.load(Ordering::SeqCst) {
            icons.push("\x1b[31m\x1b[0m")
        }
        if agent.incognito {
            icons.push("");
        }
        if agent.yolo {
            icons.push("\x1b[33m\x1b[0m");
        }
        let status = icons.join(" ");

        let readline = rl.readline(&format!(
            "\x1b[35m✶\x1b[0m qsi · {}{}\x1b[38;5;173m{}\x1b[0m ({}) · \x1b[31m↑\x1b[0m{} (\x1b[31m↟\x1b[0m{}) \x1b[34m↓\x1b[0m{}\n\x1b[35m⟩\x1b[0m ",
            if status.is_empty() { "".to_string() } else { format!("{status} · ") },
            agent.save.as_ref().map_or("".to_string(), |n| format!("{n} · ")),
            agent.model,
            agent.thinking,
            render::tokens(agent.usage.input_tokens),
            render::tokens(agent.usage.cache_creation_input_tokens),
            render::tokens(agent.usage.output_tokens),
        ));
        utils::set_echo(false);

        match readline {
            Ok(prompt) => {
                if prompt.trim().is_empty() {
                    continue;
                }
                let res = prompt::prompt(&prompt, &mut agent, renderer.clone());

                if let Err(e) = res {
                    renderer.lock().unwrap().set_status(None);
                    renderer.lock().unwrap().write_error(&format!("{e:#}"));
                }
            }
            Err(ReadlineError::Eof) => break,
            _ => continue,
        }
    }

    Ok(())
}
