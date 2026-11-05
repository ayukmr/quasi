use std::{collections::HashMap, path::PathBuf};
use std::fs;
use std::process::Command;

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    pub params: Vec<(&'static str, &'static str, &'static str)>,
    pub required: Vec<&'static str>,
    pub dangerous: bool,
    pub call: fn(HashMap<String, Value>) -> Result<String>,
    pub render: fn(HashMap<String, Value>) -> Result<String>,
}

impl Tool {
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

pub fn tools() -> Vec<Tool> {
    vec![
        Tool {
            name: "read",
            description: "Read the contents of a file. For text files, defaults to first 2000 lines. Use offset/limit for large files.",
            params: vec![
                ("path", "string", "Path to the file to read (relative or absolute)"),
                ("offset", "number", "Line number to start reading from (1-indexed)"),
                ("limit", "number", "Maximum number of lines to read"),
            ],
            required: vec!["path"],
            dangerous: false,
            call: read,
            render: render_read,
        },
        Tool {
            name: "write",
            description: "Write content to a file. Creates the file if it doesn't exist, overwrites if it does. Automatically creates parent directories.",
            params: vec![
                ("path", "string", "Path to the file to write (relative or absolute)"),
                ("content", "string", "Content to write to the file"),
            ],
            required: vec!["path", "content"],
            dangerous: true,
            call: write,
            render: render_write,
        },
        Tool {
            name: "edit",
            description: "Edit a file by replacing exact text. The oldText must match exactly (including whitespace). Use this for precise, surgical edits.",
            params: vec![
                ("path", "string", "Path to the file to edit (relative or absolute)"),
                ("old_text", "string", "Exact text to find and replace (must match exactly)"),
                ("new_text", "string", "New text to replace the old text with"),
            ],
            required: vec!["path", "old_text", "new_text"],
            dangerous: true,
            call: edit,
            render: render_edit,
        },
        Tool {
            name: "bash",
            description: "Execute a bash command in the current working directory. Returns stdout and stderr.",
            params: vec![
                ("command", "string", "Bash command to execute"),
            ],
            required: vec!["command"],
            dangerous: false,
            call: bash,
            render: render_bash,
        },
    ]
}

fn read(args: HashMap<String, Value>) -> Result<String> {
    let path = args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?;
    let offset = args.get("offset").map(|o| o.as_u64().context("offset: wrong type")).transpose()?.unwrap_or(1) as usize;
    let limit = args.get("limit").map(|o| o.as_u64().context("limit: wrong type")).transpose()?.unwrap_or(2000) as usize;

    let content = fs::read_to_string(path)?;
    let lines = content.lines().skip(offset.saturating_sub(1));
    let out: Vec<&str> = lines.take(limit).collect();

    Ok(out.join("\n"))
}

fn render_read(args: HashMap<String, Value>) -> Result<String> {
    let path = args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?;
    let offset = args.get("offset").map(|o| o.as_u64().context("offset: wrong type")).transpose()?;
    let limit = args.get("limit").map(|o| o.as_u64().context("limit: wrong type")).transpose()?;

    Ok(format!(
        "reading {path}{}{}",
        offset.map_or(String::new(), |o| format!(" @ {o}")),
        limit.map_or(String::new(), |l| format!(" (limit: {l})")),
    ))
}

fn write(args: HashMap<String, Value>) -> Result<String> {
    let path = args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?;
    let content = args.get("content").context("content: omitted")?.as_str().context("content: wrong type")?;

    fs::create_dir_all(PathBuf::from(path).parent().unwrap())?;
    fs::write(path, content)?;

    Ok(format!("successfully wrote to {path}"))
}

fn render_write(args: HashMap<String, Value>) -> Result<String> {
    let path = args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?;
    let content = args.get("content").context("content: omitted")?.as_str().context("content: wrong type")?;

    let diff_new = content.lines().map(|l| format!("\x1b[32m+ {l}\x1b[0m")).collect::<Vec<_>>().join("\n");

    Ok(format!("writing to {path}:\n{diff_new}"))
}

fn edit(args: HashMap<String, Value>) -> Result<String> {
    let path = args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?;
    let old = args.get("old_text").context("old_text: omitted")?.as_str().context("old_text: wrong type")?;
    let new = args.get("new_text").context("new_text: omitted")?.as_str().context("new_text: wrong type")?;

    let content = fs::read_to_string(path)?;
    let replaced = content.replacen(old, new, 1);

    if content == replaced {
        bail!("old_text does not exist in file");
    }

    fs::write(path, replaced)?;

    Ok(format!("successfully replaced text in {path}"))
}

fn render_edit(args: HashMap<String, Value>) -> Result<String> {
    let path = args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?;
    let old = args.get("old_text").context("old_text: omitted")?.as_str().context("old_text: wrong type")?;
    let new = args.get("new_text").context("new_text: omitted")?.as_str().context("new_text: wrong type")?;

    let content = fs::read_to_string(path)?;
    let start = content.find(old).context("old_text does not exist in file")?;
    let end = start + old.len();

    let lines_start = content[..start].rfind('\n').map_or(0, |i| i + 1);
    let lines_end = content[end..].find('\n').map_or(content.len(), |i| end + i);

    let range = &content[lines_start..lines_end];

    let diff_old = range.replacen(old, &format!("\x1b[30;41m{old}\x1b[31;49m"), 1).lines().map(|l| format!("\x1b[31m- {l}\x1b[0m")).collect::<Vec<_>>().join("\n");
    let diff_new = range.replacen(old, &format!("\x1b[30;42m{new}\x1b[32;49m"), 1).lines().map(|l| format!("\x1b[32m+ {l}\x1b[0m")).collect::<Vec<_>>().join("\n");

    Ok(format!("editing {path}:\n{diff_old}\n{diff_new}"))
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

fn bash(args: HashMap<String, Value>) -> Result<String> {
    let command = args.get("command").context("command: omitted")?.as_str().context("command: wrong type")?;

    let output = Command::new("sandbox-exec")
        .args(["-p", SANDBOX])
        .args(["bash", "-c", command])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    Ok(format!("stdout: {stdout}\nstderr: {stderr}"))
}

fn render_bash(args: HashMap<String, Value>) -> Result<String> {
    let command = args.get("command").context("command: omitted")?.as_str().context("command: wrong type")?;

    Ok(format!("$ {command}"))
}
