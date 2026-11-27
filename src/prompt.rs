use std::sync::{Arc, Mutex};
use std::{env, fs};
use std::path::{Path, PathBuf};

use anyhow::Result;
use rustyline::history::MemHistory;
use rustyline::{Config, Context, EditMode, Editor, Helper, Highlighter, Hinter, Validator};
use rustyline::completion::{Completer, Pair};

use crate::{Agent, Renderer, utils};

const MODELS: &[&str] = &["fable-5-1", "opus-5-5", "sonnet-5-5", "haiku-5-5"];
const THINKS: &[&str] = &["low", "medium", "high", "xhigh", "max"];

const CMDS: &[(&str, Option<&[&str]>)] = &[
    ("/save", None),
    ("/model", Some(MODELS)),
    ("/think", Some(THINKS)),
    ("/incognito", None),
    ("/yolo", None),
    ("/reload", None),
];

pub fn editor() -> Result<Editor<Complete, MemHistory>> {
    let config = Config::builder().edit_mode(EditMode::Vi).build();

    let mut rl = Editor::with_config(config)?;
    rl.set_helper(Some(Complete));

    Ok(rl)
}

pub fn prompt(prompt: &str, agent: &mut Agent, renderer: Arc<Mutex<Renderer>>) -> Result<()> {
    if let Some(name) = prompt.strip_prefix("/save ") {
        agent.update_save(name)?;
        renderer.lock().unwrap().write_success(&format!("saving to {name}"));
    } else if let Some(model) = prompt.strip_prefix("/model ") {
        if MODELS.contains(&model) {
            agent.model = model.to_string();
            renderer.lock().unwrap().write_success(&format!("model set to {model}"));
        } else {
            renderer.lock().unwrap().write_error(&format!("invalid model id '{model}'"));
        }
    } else if let Some(think) = prompt.strip_prefix("/think ") {
        if THINKS.contains(&think) {
            agent.thinking = think.to_string();
            renderer.lock().unwrap().write_success(&format!("thinking set to {think}"));
        } else {
            renderer.lock().unwrap().write_error(&format!("invalid thinking level '{think}'"));
        }
    } else if prompt == "/incognito" {
        agent.incognito = !agent.incognito;
        if agent.incognito {
            renderer.lock().unwrap().write_success("incognito enabled");
        } else {
            renderer.lock().unwrap().write_error("incognito disabled");
        }
    } else if prompt == "/yolo" {
        agent.yolo = !agent.yolo;
        if agent.yolo {
            renderer.lock().unwrap().write_success("yolo enabled");
        } else {
            renderer.lock().unwrap().write_error("yolo disabled");
        }
    } else if prompt == "/reload" {
        agent.reload()?;
        renderer.lock().unwrap().write_success("reloaded tools");
    } else {
        agent.prompt(&prompt)?;
        if !agent.incognito {
            agent.save()?;
        }
    }

    Ok(())
}

#[derive(Helper, Hinter, Highlighter, Validator)]
pub struct Complete;

impl Completer for Complete {
    type Candidate = Pair;

    fn complete(&self, line: &str, pos: usize, _: &Context<'_>) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[0..line.find(' ').unwrap_or(0)];
        let up_to = &line[..pos];
        let start = up_to.rfind(' ').map_or(0, |i| i + 1);
        let cur = &line[start..pos];

        if line[start..].starts_with('@') {
            let prefix: &str = &line[start + 1..pos];

            let raw = PathBuf::from(prefix);
            let (base, name_prefix) = if prefix.is_empty() || prefix.ends_with("/") {
                (raw, "")
            } else {
                (
                    raw.parent().unwrap_or(Path::new(".")).to_path_buf(),
                    raw.file_name().and_then(|s| s.to_str()).unwrap_or(""),
                )
            };

            let mut dir = utils::expand_tilde_path(&base);
            if dir.is_relative() {
                let cwd = env::current_dir()?;
                dir = Path::join(cwd.as_path(), dir);
            }

            let Ok(entries) = fs::read_dir(dir) else {
                return Ok((start + 1, Vec::new()));
            };

            let mut files: Vec<Pair> = entries
                .filter_map(Result::ok)
                .filter(|e| e.file_name().to_string_lossy().starts_with(name_prefix))
                .map(|e| {
                    let p = e.path();

                    let mut s = base.join(p.file_name().unwrap()).to_string_lossy().to_string();
                    if p.is_dir() {
                        s += "/";
                    }

                    Pair { display: s.clone(), replacement: s }
                })
                .collect();
            files.sort_by_key(|p| p.display.len());

            return Ok((start + 1, files));
        }

        let cmds: Vec<&str> = CMDS.iter().map(|(c, _)| *c).collect();

        let poss = if let Some((_, Some(arg))) = CMDS.iter().find(|(c, _)| *c == prefix) {
            arg
        } else if !up_to.contains(' ') && up_to.starts_with('/') {
            cmds.as_slice()
        } else {
            &[]
        };

        let mut matches: Vec<Pair> = poss
            .iter()
            .filter(|c| c.starts_with(cur))
            .map(|c| Pair { display: c.to_string(), replacement: c.to_string() })
            .collect();
        matches.sort_by_key(|p| p.display.len());

        Ok((start, matches))
    }
}
