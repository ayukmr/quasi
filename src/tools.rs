use crate::sse::{Content, FileSource, StrContent};
use crate::{Renderer, render, utils};

use std::fs;
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, path::PathBuf};
use std::process::{Command, Output};

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use base64::prelude::BASE64_STANDARD;
use infer::MatcherType;
use mlua::{Function, Lua, LuaSerdeExt, Table};
use serde_json::{Value, json};

pub struct Tool {
    pub name: String,
    pub description: String,
    pub params: Vec<(String, String, String)>,
    pub required: Vec<String>,
    pub dangerous: bool,
    pub call: Box<dyn Fn(HashMap<String, Value>) -> Result<Vec<Content>>>,
    pub render: Box<dyn Fn(HashMap<String, Value>) -> Result<String>>,
}

impl Tool {
    pub fn create(
        name: &str,
        description: &str,
        params: Vec<(&str, &str, &str)>,
        required: Vec<&str>,
        dangerous: bool,
        call: Box<dyn Fn(HashMap<String, Value>) -> Result<Vec<Content>>>,
        render: Box<dyn Fn(HashMap<String, Value>) -> Result<String>>,
    ) -> Self {
        Self {
            name: name.to_string(),
            description: description.to_string(),
            params: params.iter().map(|p| (p.0.to_string(), p.1.to_string(), p.2.to_string())).collect(),
            required: required.iter().map(|r| r.to_string()).collect(),
            dangerous,
            call,
            render,
        }
    }

    pub fn to_json(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "eager_input_streaming": true,
            "input_schema": {
                "type": "object",
                "properties": self.params.iter()
                    .map(|(k, t, d)| (k, json!({ "type": t, "description": d })))
                    .collect::<HashMap<_, _>>(),
                "required": self.required,
            }
        })
    }
}

fn lua_err(e: mlua::Error) -> anyhow::Error {
    anyhow!("{e}")
}

pub fn tools(renderer: Arc<Mutex<Renderer>>) -> Result<Vec<Tool>> {
    let mut tools = Vec::new();

    if let Ok(entries) = fs::read_dir(utils::tools_dir()) {
        for entry in entries {
            let path = entry?.path();
            if path.extension().is_none_or(|e| e != "lua") {
                continue;
            }

            let path_s = path.to_str().unwrap();

            let res = (|| -> Result<()> {
                let lua = Lua::new();
                lua.load(path.as_path()).exec().map_err(lua_err)?;

                let globals = lua.globals();

                let name: String = globals.get("name").map_err(lua_err)?;
                let description: String = format!(
                    "{} (Lua tool from {path_s})",
                    globals.get::<String>("description").map_err(lua_err)?,
                );

                let params_t: Table = globals.get("params").map_err(lua_err)?;
                let params = params_t
                    .sequence_values::<Table>()
                    .map(|r| {
                        let r = r?;
                        Ok((r.get(1)?, r.get(2)?, r.get(3)?))
                    })
                    .collect::<mlua::Result<Vec<(String, String, String)>>>().map_err(lua_err)?;

                let required: Vec<String> = globals.get("required").map_err(lua_err)?;
                let dangerous: bool = globals.get("dangerous").map_err(lua_err)?;

                let call: Function = globals.get("call").map_err(lua_err)?;
                let render: Function = globals.get("render").map_err(lua_err)?;

                let lua_c = lua.clone();
                let call_box = Box::new(move |args| {
                    let arg = lua_c.to_value(&args).map_err(lua_err)?;

                    let out = call.call(arg).map_err(lua_err)?;
                    match lua_c.from_value(out).map_err(lua_err)? {
                        StrContent::Str(s) => Ok(utils::text(s)),
                        StrContent::Content(c) => Ok(c),
                    }
                });

                let lua_r = lua.clone();
                let render_box = Box::new(move |args| {
                    let arg = lua_r.to_value(&args).map_err(lua_err)?;
                    render.call(arg).map_err(lua_err)
                });

                tools.push(
                    Tool {
                        name,
                        description,
                        params,
                        required,
                        dangerous,
                        call: call_box,
                        render: render_box,
                    }
                );

                Ok(())
            })();

            if let Err(e) = res {
                renderer.lock().unwrap().write_error(&format!("error loading {path_s}: {e:#}"));
            }
        }
    }

    tools.push(
        Tool::create(
            "read",
            "Read the contents of a text/image/PDF file. For text files, defaults to first 2000 lines. For image/PDF files, reads the entire file. Use offset/limit for large files.",
            vec![
                ("path", "string", "Path to the file to read (relative or absolute)"),
                ("offset", "number", "Line number to start reading from (1-indexed). Do not specify for images"),
                ("limit", "number", "Maximum number of lines to read. Do not specify for images"),
            ],
            vec!["path"],
            false,
            Box::new(read),
            Box::new(render_read),
        )
    );

    tools.push(
        Tool::create(
            "write",
            "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. Automatically creates parent directories.",
            vec![
                ("path", "string", "Path to the file to write (relative or absolute)"),
                ("content", "string", "Content to write to the file"),
            ],
            vec!["path", "content"],
            true,
            Box::new(write),
            Box::new(render_write),
        )
    );

    tools.push(
        Tool::create(
            "edit",
            "Edit a file by replacing exact text. The oldText must match exactly (including whitespace). Use this for precise, surgical edits.",
            vec![
                ("path", "string", "Path to the file to edit (relative or absolute)"),
                ("old_text", "string", "Exact text to find and replace (must match exactly)"),
                ("new_text", "string", "New text to replace the old text with"),
            ],
            vec!["path", "old_text", "new_text"],
            true,
            Box::new(edit),
            Box::new(render_edit),
        )
    );

    tools.push(
        Tool::create(
            "bash",
            "Execute a read-only bash command. Read-only sandbox, any file write fails with 'Operation not permitted'. Returns stdout and stderr.",
            vec![
                ("command", "string", "Bash command to execute"),
            ],
            vec!["command"],
            false,
            Box::new(bash),
            Box::new(render_bash),
        )
    );

    tools.push(
        Tool::create(
            "bash_write",
            "Execute a bash command that needs write access. Use only for commands with side effect: builds, tests, installs. Never use it to create or change file contents (no heredocs, no >, no >>, no tee, no sed -i), use edit/write instead. Returns stdout and stderr.",
            vec![
                ("command", "string", "Bash command to execute"),
            ],
            vec!["command"],
            true,
            Box::new(bash_write),
            Box::new(render_bash_write),
        )
    );

    Ok(tools)
}

fn read(args: HashMap<String, Value>) -> Result<Vec<Content>> {
    let path = utils::expand_tilde(args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?);
    let offset = args.get("offset").map(|o| o.as_u64().context("offset: wrong type")).transpose()?;
    let limit = args.get("limit").map(|o| o.as_u64().context("limit: wrong type")).transpose()?;

    let content = fs::read(path)?;

    let media = infer::get(&content).and_then(|t| match (t.matcher_type(), t.mime_type()) {
        (MatcherType::Image, m) => Some(("image", m)),
        (MatcherType::Archive, m @ "application/pdf") => Some(("document", m)),
        _ => None,
    });

    if let Some((ty, mime)) = media {
        if offset.is_some() || limit.is_some() {
            bail!("offset or limit specified for binary file");
        }

        let source = FileSource {
            kind: "base64".to_string(),
            media_type: mime.to_string(),
            data: BASE64_STANDARD.encode(content),
        };

        Ok(vec![if ty == "image" {
            Content::Image { source }
        } else {
            Content::Document { source }
        }])
    } else {
        let offset = offset.unwrap_or(1) as usize;
        let limit = limit.unwrap_or(2000) as usize;
        let content = String::from_utf8_lossy(&content);

        let all: Vec<&str> = content.lines().collect();
        let total = all.len();

        if total == 0 {
            bail!("file is empty");
        }
        if offset > total {
            bail!("offset {offset} is past total lines {total}");
        }

        let out: Vec<&str> = all.into_iter().skip(offset.saturating_sub(1)).take(limit).collect();
        let end = offset + out.len() - 1;

        Ok(utils::text(format!("{}\n[lines {offset}-{end} of {total}]", out.join("\n"))))
    }
}

fn render_read(args: HashMap<String, Value>) -> Result<String> {
    let path = utils::expand_tilde(args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?);
    let offset = args.get("offset").map(|o| o.as_u64().context("offset: wrong type")).transpose()?;
    let limit = args.get("limit").map(|o| o.as_u64().context("limit: wrong type")).transpose()?;

    Ok(format!(
        "\x1b[1mreading\x1b[0m \x1b[36m{}\x1b[0m{}{}",
        path.to_string_lossy(),
        offset.map_or(String::new(), |o| format!(" @ {o}")),
        limit.map_or(String::new(), |l| format!(" (limit: {l})")),
    ))
}

fn write(args: HashMap<String, Value>) -> Result<Vec<Content>> {
    let path = &utils::expand_tilde(args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?);
    let content = args.get("content").context("content: omitted")?.as_str().context("content: wrong type")?;

    fs::create_dir_all(PathBuf::from(path).parent().unwrap())?;
    fs::write(path, content)?;

    Ok(utils::text(format!("successfully wrote to {}", path.to_string_lossy())))
}

fn render_write(args: HashMap<String, Value>) -> Result<String> {
    let path = &utils::expand_tilde(args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?);
    let content = args.get("content").context("content: omitted")?.as_str().context("content: wrong type")?;

    let syn = render::bat(content, &["-n", "--file-name", &path.to_string_lossy()]);

    Ok(format!("\x1b[1mwriting\x1b[0m \x1b[36m{}\x1b[0m:\n{}", path.to_string_lossy(), syn))
}

fn edit(args: HashMap<String, Value>) -> Result<Vec<Content>> {
    let path = &utils::expand_tilde(args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?);
    let old = args.get("old_text").context("old_text: omitted")?.as_str().context("old_text: wrong type")?;
    let new = args.get("new_text").context("new_text: omitted")?.as_str().context("new_text: wrong type")?;

    let content = fs::read_to_string(path)?;
    let replaced = content.replacen(old, new, 1);

    if content == replaced {
        bail!("old_text does not exist in file");
    }

    fs::write(path, replaced)?;

    Ok(utils::text(format!("successfully replaced text in {}", path.to_string_lossy())))
}

fn render_edit(args: HashMap<String, Value>) -> Result<String> {
    let path = &utils::expand_tilde(args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?);
    let old = args.get("old_text").context("old_text: omitted")?.as_str().context("old_text: wrong type")?;
    let new = args.get("new_text").context("new_text: omitted")?.as_str().context("new_text: wrong type")?;

    let content = fs::read_to_string(path)?;
    let start = content.find(old).context("old_text does not exist in file")?;
    let end = start + old.len();

    let lines_start = content[..start].rfind('\n').map_or(0, |i| i + 1);
    let lines_end = content[end..].find('\n').map_or(content.len(), |i| end + i);

    let prefix = &content[lines_start..start];
    let suffix = &content[end..lines_end];

    let diff_old = format_diff(prefix, suffix, old, "-", "31", "41");
    let diff_new = format_diff(prefix, suffix, new, "+", "32", "42");

    Ok(format!("\x1b[1mediting\x1b[0m \x1b[36m{}\x1b[0m:\n{diff_old}\n{diff_new}", path.to_string_lossy()))
}

fn format_diff(prefix: &str, suffix: &str, mid: &str, marker: &str, fg: &str, hl: &str) -> String {
    let segments: Vec<&str> = mid.split('\n').collect();
    let last = segments.len().saturating_sub(1);

    segments
        .iter()
        .enumerate()
        .map(|(i, seg)| {
            let mut s = String::new();

            if i == 0 {
                s += prefix;
            }
            s += &format!("\x1b[30;{hl}m{seg}\x1b[{fg};49m");
            if i == last {
                s += suffix;
            }

            format!("\x1b[{fg}m{marker} {s}\x1b[0m")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn bash_helper(output: Output) -> Vec<Content> {
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().map_or(
        "terminated by signal".to_string(),
        |c| format!("status code: {c}"),
    );

    utils::text(format!("stdout: {stdout}\nstderr: {stderr}\n{code}"))
}

const SANDBOX: &str = r#"(version 1)
(allow default)

(deny file-write*)

(allow file-write-data
  (literal "/dev/null")
  (literal "/dev/zero")
  (literal "/dev/dtracehelper")
  (regex #"^/dev/tty")
  (regex #"^/dev/fd/"))"#;

fn bash(args: HashMap<String, Value>) -> Result<Vec<Content>> {
    let command = args.get("command").context("command: omitted")?.as_str().context("command: wrong type")?;

    let output = Command::new("sandbox-exec")
        .args(["-p", SANDBOX])
        .args(["bash", "-c", command])
        .output()?;

    Ok(bash_helper(output))
}

fn render_bash(args: HashMap<String, Value>) -> Result<String> {
    let command = args.get("command").context("command: omitted")?.as_str().context("command: wrong type")?;

    Ok(format!("\x1b[1m$\x1b[0m {}", render::bat(command, &["-l", "bash"])))
}

fn bash_write(args: HashMap<String, Value>) -> Result<Vec<Content>> {
    let command = args.get("command").context("command: omitted")?.as_str().context("command: wrong type")?;

    let output = Command::new("bash")
        .args(["-c", command])
        .output()?;

    Ok(bash_helper(output))
}

fn render_bash_write(args: HashMap<String, Value>) -> Result<String> {
    let command = args.get("command").context("command: omitted")?.as_str().context("command: wrong type")?;

    Ok(format!("\x1b[1;31m$\x1b[0m {}", render::bat(command, &["-l", "bash"])))
}
