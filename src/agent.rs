use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};

use crate::oauth::Credentials;
use crate::render::Renderer;
use crate::sse::{self, Body, Content, Message, Role, StopReason};
use crate::tools::{self, Tool};

const SYSTEM: &str = "You are an expert coding assistant. You help users with coding tasks by reading files, executing commands, editing code, and writing new files.

Available tools:
- read: Read file contents
- bash: Execute bash commands
- edit: Make surgical edits to files
- write: Create or overwrite files

Guidelines:
- Use read to examine files before editing
- Use edit for precise changes (old text must match exactly)
- Use write only for new files or complete rewrites
- Use bash for file operations like ls, grep, find
- Do not use bash when another tool would work
- Bash is sandboxed for file writing, do not try to get around it
- Do not be too eager with writing files - ask the user if it is implied but not said
- When summarizing your actions, output plain text directly - do NOT use cat or bash to display what you did
- Be concise in your responses, do not state multiple solutions unless asked
- Show file paths clearly when working with files";

pub struct Agent {
    renderer: Arc<Mutex<Renderer>>,
    creds: Credentials,
    tools: Vec<Tool>,
    messages: Vec<Message>,
}

impl Agent {
    pub fn new(renderer: Arc<Mutex<Renderer>>) -> Result<Self> {
        Ok(Self {
            renderer,
            creds: Credentials::get()?,
            tools: tools::tools(),
            messages: Vec::new(),
        })
    }

    fn body(&self) -> Body {
        Body {
            model: "claude-sonnet-5-5".to_string(),
            max_tokens: 32_768,
            stream: true,
            system: vec![
                Content::Text { text: "You are Claude Code, Anthropic's official CLI for Claude.".to_string(), cache_control: None },
                Content::Text { text: SYSTEM.to_string(), cache_control: None },
            ],
            tools: self.tools.iter().map(Tool::to_json).collect(),
            messages: self.messages.clone(),
        }
    }

    pub fn prompt(&mut self, query: &str) -> Result<()> {
        self.messages.push(
            Message {
                role: Role::User,
                content: vec![Content::Text { text: query.to_string(), cache_control: None }],
            }
        );

        let mut resp = sse::req(&self.body(), &self.creds, self.renderer.clone())?;

        loop {
            if !matches!(resp.stop_reason, StopReason::ToolUse) {
                break;
            }

            let mut tool_res = vec![];

            for c in &resp.content {
                if let Content::ToolUse { id, name, input } = c {
                    let tool = self.tools.iter().find(|t| t.name == *name).context("invalid tool call")?;

                    self.renderer.lock().unwrap().write_line(
                        &format!(
                            "\x1b[1m+ {name}({})\x1b[0m",
                            input.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<String>>().join(", "),
                        ),
                    );

                    if !tool.dangerous || self.renderer.lock().unwrap().confirm() {
                        let resp = (tool.call)(input.clone());
                        let err = resp.is_err();

                        tool_res.push(
                            Content::ToolResult {
                                tool_use_id: id.to_string(),
                                content: resp.unwrap_or_else(|e| format!("error: {}", e)),
                                is_error: err,
                                cache_control: None,
                            },
                        );
                    } else {
                        tool_res.push(
                            Content::ToolResult {
                                tool_use_id: id.to_string(),
                                content: "error: tool denied by user".to_string(),
                                is_error: true,
                                cache_control: None,
                            },
                        );
                    }
                }
            }

            self.messages.push(Message { role: Role::Assistant, content: resp.content });
            self.messages.push(Message { role: Role::User, content: tool_res });

            resp = sse::req(&self.body(), &self.creds, self.renderer.clone())?;
        }

        self.messages.push(Message { role: Role::Assistant, content: resp.content });

        Ok(())
    }
}
