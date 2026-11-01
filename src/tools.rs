use std::collections::HashMap;
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
        },
        Tool {
            name: "bash",
            description: "Execute a bash command in the current working directory. Returns stdout and stderr. Optionally provide a timeout in seconds.",
            params: vec![
                ("command", "string", "Bash command to execute"),
                ("timeout", "number", "Timeout in seconds (optional, no default timeout)"),
            ],
            required: vec!["command"],
            dangerous: false,
            call: bash,
        },
    ]
}

fn read(args: HashMap<String, Value>) -> Result<String> {
    let path = args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?;
    let offset = args.get("offset").map(|o| o.as_u64().context("offset: wrong type")).transpose()?.unwrap_or(1) as usize;
    let limit = args.get("limit").map(|o| o.as_u64().context("limit: wrong type")).transpose()?.unwrap_or(2000) as usize;

    let data = fs::read_to_string(path)?;
    let lines = data.lines().skip(offset.saturating_sub(1));
    let out: Vec<&str> = lines.take(limit).collect();

    Ok(out.join("\n"))
}

fn write(args: HashMap<String, Value>) -> Result<String> {
    let path = args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?;
    let content = args.get("content").context("content: omitted")?.as_str().context("content: wrong type")?;

    fs::write(path, content)?;

    Ok(format!("successfully wrote to {path}"))
}

fn edit(args: HashMap<String, Value>) -> Result<String> {
    let path = args.get("path").context("path: omitted")?.as_str().context("path: wrong type")?;
    let old = args.get("old_text").context("old_text: omitted")?.as_str().context("old_text: wrong type")?;
    let new = args.get("new_text").context("new_text: omitted")?.as_str().context("new_text: wrong type")?;

    let data = fs::read_to_string(path)?;
    let replaced = data.replacen(old, new, 1);

    if data == replaced {
        bail!("old_text does not exist in file");
    }

    fs::write(path, replaced)?;

    Ok(format!("successfully replaced text in {path}"))
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

// TODO use timeout
fn bash(args: HashMap<String, Value>) -> Result<String> {
    let command = args.get("command").context("command: omitted")?.as_str().context("command: wrong type")?;
    let timeout = args.get("timeout").map(|o| o.as_u64().context("timeout: wrong type")).transpose()?.unwrap_or(1) as usize;

    let output = Command::new("sandbox-exec")
        .args(["-p", SANDBOX])
        .args(["bash", "-c", command])
        .output()?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    Ok(format!("stdout: {}\nstderr: {}", stdout, stderr))
}
